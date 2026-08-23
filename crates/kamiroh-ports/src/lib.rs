//! kamiroh's ports.
//!
//! Traits defined by the core: driven ports are implemented by adapters
//! (`kamiroh-adapter-*`), driving ports are consumed by embedding applications
//! and agent harnesses. Adapters depend on `kamiroh-domain` + this crate only —
//! never on the application layer — so the hexagon's dependency arrows are
//! compiler-enforced.

use std::ops::Add;
use std::time::Duration;

use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::deadline::FailureCause;
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::protocol::TurnError;
use kamiroh_domain::vocabulary::{Message, Request};

/// The claimed sender name on a batch receipt (`ARCHITECTURE.md`,
/// decision 29): the endpoint's own acknowledgment that a fan-out batch
/// arrived, before any actor was consulted. No actor sends it — the name
/// is a convention, and deliberately NOT a reserved namespace: names are
/// claims, never proof (decision 2), and an admitted endpoint could
/// already forge a plain `Ack` under any name it likes, so a
/// convention-named receipt adds no capability anyone lacks. The
/// collision (a real actor installed under this name) is accepted and
/// documented rather than prevented.
pub const BATCH_RECEIPT_NAME: &str = "mailroom";

/// The full claimed sender address of a batch receipt from `endpoint`.
pub fn batch_receipt_sender(endpoint: &EndpointId) -> Address {
    Address::new(
        endpoint.clone(),
        ActorName::new(BATCH_RECEIPT_NAME).expect("the sentinel is a valid name"),
    )
}

/// An inbound delivery as witnessed by the transport.
///
/// `from.endpoint` is transport-proven; `from.name` is claimed by the remote
/// runtime. `to` is the local actor the message is addressed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub from: Address,
    pub to: Address,
    pub message: Message,
}

/// Driven port: carries vocabulary messages between actors.
///
/// Implemented by `kamiroh-adapter-iroh` for real conversations (short- or
/// long-lived) and by `kamiroh-adapter-memory` for in-process tests.
pub trait Transport {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Open (or reuse) a conversation with the actor at `to` and send
    /// `message` as `from`.
    ///
    /// Implementations' futures must be `Send`: these ports are crossed by
    /// multi-threaded runtimes by design (ARCHITECTURE.md, decision 15).
    fn send(
        &mut self,
        from: &Address,
        to: &Address,
        message: Message,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;

    /// Open exchanges with several actors at ONE endpoint in a single
    /// send (`ARCHITECTURE.md`, decision 29). The signature is the rules:
    /// one `EndpointId` makes a mixed-endpoint batch unrepresentable
    /// (R1), and taking a [`Request`] rather than a [`Message`] means
    /// only opening turns fan out (Q2).
    ///
    /// The transport delivers `Turn::Open { request }` to each *bound*
    /// name — unbound names are dropped silently, disclosing nothing —
    /// and answers with a single **batch receipt**: an ordinary
    /// `Message::Ack` for `request.id`, claimed-from
    /// [`batch_receipt_sender`], sent at endpoint arrival, *before* any
    /// admission is consulted and even when every name is unbound. It
    /// promises "the batch reached the endpoint" — deliberately less
    /// than the pairwise ack's reached-and-admitted (decision 4,
    /// qualified by decision 29).
    fn open_many(
        &mut self,
        from: &Address,
        to_endpoint: &EndpointId,
        to_names: &[ActorName],
        request: Request,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;
}

/// Driving port: the inbound surface handed to an embedding application or an
/// agent's harness — messages arriving for its dedicated actor.
pub trait Inbox {
    /// The next delivery, or `None` when the conversation source is closed.
    ///
    /// Implementations' futures must be `Send`: these ports are crossed by
    /// multi-threaded runtimes by design (ARCHITECTURE.md, decision 15).
    fn next(&mut self) -> impl std::future::Future<Output = Option<Delivery>> + Send;
}

/// Driven port on the app side of the hexagon: **the party behind an actor**
/// (`ARCHITECTURE.md`, decision 16). The embedding application implements
/// this, one per actor; kamiroh drives it — push, not pull.
///
/// The signature *is* the atomicity contract (decision 17): one incoming turn
/// → one atomic state change (guarded by `&mut self` and the runtime's
/// per-actor serialization) → at most one outgoing turn, emitted by the
/// runtime only after this method returns, i.e. after the state has settled.
///
/// Contract for the return value, enforced by the runtime's `TurnState`:
/// - Incoming `Open`/`Continue` (a request is posed): return `Some(turn)`
///   whose response half answers it — `Continue` to keep the exchange going,
///   `Close` to conclude it.
/// - Incoming `Close` (nothing asked): return `None`; the exchange is over.
pub trait Party {
    fn on_turn(
        &mut self,
        from: &Address,
        turn: kamiroh_domain::vocabulary::Turn,
    ) -> impl std::future::Future<Output = Option<kamiroh_domain::vocabulary::Turn>> + Send;

    /// The exchange with `from` has failed — a deadline elapsed, the peer
    /// vanished, or this side's operator revoked the peer
    /// (`ARCHITECTURE.md`, decision 25). The party must act on this (clean
    /// up, abandon work in progress, perhaps reopen); no further turn of
    /// the dead exchange will ever arrive. The conversation itself
    /// survives: a fresh exchange with the same peer may follow. Exception
    /// to "perhaps reopen": on [`FailureCause::Revoked`] do **not** reopen
    /// unprompted — the peer was cut off deliberately by this side's own
    /// operator, its replies will be denied at the door, and a party that
    /// retries anyway livelocks, burning one deadline per round.
    ///
    /// Default: do nothing, so existing parties keep compiling — but a party
    /// embedded for unattended operation should override this.
    fn on_exchange_failed(
        &mut self,
        from: &Address,
        cause: FailureCause,
    ) -> impl std::future::Future<Output = ()> + Send {
        let _ = (from, cause);
        async {}
    }
}

/// Object-safe form of [`Party`], for runtimes hosting heterogeneous parties.
/// Blanket-implemented; implement [`Party`], not this.
pub trait DynParty: Send {
    fn on_turn_boxed<'a>(
        &'a mut self,
        from: &'a Address,
        turn: kamiroh_domain::vocabulary::Turn,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Option<kamiroh_domain::vocabulary::Turn>> + Send + 'a>,
    >;

    fn on_exchange_failed_boxed<'a>(
        &'a mut self,
        from: &'a Address,
        cause: FailureCause,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
}

impl<P: Party + Send> DynParty for P {
    fn on_turn_boxed<'a>(
        &'a mut self,
        from: &'a Address,
        turn: kamiroh_domain::vocabulary::Turn,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Option<kamiroh_domain::vocabulary::Turn>> + Send + 'a>,
    > {
        Box::pin(self.on_turn(from, turn))
    }

    fn on_exchange_failed_boxed<'a>(
        &'a mut self,
        from: &'a Address,
        cause: FailureCause,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(self.on_exchange_failed(from, cause))
    }
}

/// Driven port: time, as a device (`ARCHITECTURE.md`, decision 24).
///
/// The domain never reads a clock; deadlines are data
/// ([`Deadlines`](kamiroh_domain::deadline::Deadlines)) and the app layer's
/// timer *choreography* — arm on send, cancel on arrival, re-arm per round —
/// is written once against this port, so the same code runs under the real
/// runtimes' tokio-backed timer and under a hand-advanced test timer where a
/// 60-second deadline fires in microseconds.
///
/// Shaped as a timer, not a clock-reader: what the app layer needs is "wake
/// me at this deadline," never a pollable now-loop.
pub trait Timer {
    /// A point on this timer's clock. Test timers own virtual instants;
    /// tokio-backed timers use tokio's.
    type Instant: Copy + Ord + Send + Sync + 'static + Add<Duration, Output = Self::Instant>;

    fn now(&self) -> Self::Instant;

    /// Complete when the timer's clock reaches `deadline`.
    ///
    /// Implementations' futures must be `Send` (decision 15).
    fn sleep_until(&self, deadline: Self::Instant) -> impl std::future::Future<Output = ()> + Send;
}

/// Driven port: the transport's positive evidence that a peer endpoint is
/// gone (`ARCHITECTURE.md`, decision 27; rule R8 of the mapping session:
/// transports must report what they know).
///
/// What counts as evidence is the transport's business — a QUIC connection
/// closed by the peer, timed out, or reset; a simulated kill in the memory
/// adapter. What is deliberately *not* evidence: silence (that is what
/// deadlines are for) and closes this side initiated itself (cache
/// maintenance is not a death). Consumers fail the affected *exchanges* at
/// once and leave the *conversation* standing — a conversation spans
/// connections.
pub trait DeathWatch {
    /// The next endpoint known dead, or `None` when the watch is closed.
    ///
    /// Implementations' futures must be `Send` (decision 15).
    fn next_death(&mut self) -> impl std::future::Future<Output = Option<EndpointId>> + Send;
}

/// Object-safe form of [`DeathWatch`], for runtimes holding heterogeneous
/// watches. Blanket-implemented; implement [`DeathWatch`], not this.
pub trait DynDeathWatch: Send {
    fn next_death_boxed(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<EndpointId>> + Send + '_>>;
}

impl<D: DeathWatch + Send> DynDeathWatch for D {
    fn next_death_boxed(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<EndpointId>> + Send + '_>> {
        Box::pin(self.next_death())
    }
}

/// A local operational event — something the library observed and would
/// otherwise swallow (`ARCHITECTURE.md`, decision 25).
///
/// The invariant all variants share: **never on the wire, always local.**
/// Denials stay invisible to the sender (deny-by-default demands it);
/// failures are each side's own verdict (decision 22). This surface exists
/// so unattended tests can assert positively that these things happened,
/// instead of waiting out "nothing happened" windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An exchange failed — the local verdict, already applied to the
    /// exchange's state. The affected [`Party`] is told separately (its
    /// `on_exchange_failed`); this event serves observers.
    ExchangeFailed {
        local: Address,
        peer: Address,
        cause: FailureCause,
    },
    /// A delivery from an unadmitted endpoint was denied and dropped.
    /// Local visibility only — the sender learns nothing.
    DeliveryDenied { from: Address, to: Address },
    /// An arriving turn violated alternation and was dropped.
    IllegalTurnDropped {
        from: Address,
        to: Address,
        error: TurnError,
    },
}

/// Where [`Event`]s go. Implemented by tests (collect and assert) and
/// operators (log, count); the runtimes emit into it. Blanket-implemented
/// for closures: any `FnMut(Event) + Send` observes.
///
/// Contract (decision 26): runtimes may call `on_event` while holding
/// internal locks so that no event is ever lost to a concurrent emit.
/// An observer must therefore not call back into the runtime that is
/// notifying it — observe, record, return.
pub trait Observer: Send {
    fn on_event(&mut self, event: Event);
}

impl<F: FnMut(Event) + Send> Observer for F {
    fn on_event(&mut self, event: Event) {
        self(event)
    }
}

/// Driven port: bind a local actor's [`Address`] so the transport routes
/// deliveries to it (`ARCHITECTURE.md`, decision 12).
///
/// Dropping the returned [`Inbox`] unbinds the address. The memory net
/// implements binding as registration; the Iroh adapter will implement it as
/// routing inside the endpoint.
pub trait Registry {
    type Inbox: Inbox;
    type Error: std::error::Error + Send + Sync + 'static;

    fn bind(&mut self, address: &Address) -> Result<Self::Inbox, Self::Error>;
}
