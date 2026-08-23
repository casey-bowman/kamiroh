//! The Phone — the driving handle an embedding app holds to converse
//! (`ARCHITECTURE.md`, decision 16).
//!
//! Opening a conversation is purely local (decision 11): constructing a Phone
//! allocates the conversation's turn state on this side; nothing crosses the
//! wire until the first turn is sent. The Phone enforces the alternation
//! rules — a turn can only be sent when it is legally ours to send, and
//! incoming turns are validated before the app sees their content as part of
//! a live exchange.
//!
//! The Phone also holds this side's patience (decision 22): a mandatory
//! [`Deadlines`] fixed at construction. It tracks what it is currently
//! waiting on — the ack of a sent request half, the peer's next turn — and
//! exposes the earliest due instant via [`Phone::next_due`]; the app's pump
//! races that against the inbox (see [`crate::wait`]) and reports an elapsed
//! deadline back through [`Phone::on_deadline_elapsed`], which fails the
//! exchange (decision 23). The deadlines bound *this side's waiting only*:
//! nothing about them crosses the wire, and the peer's patience is its own.
//!
//! Parties replying from inside [`Party::on_turn`](kamiroh_ports::Party) do
//! not need a Phone — the runtime sends their returned turn. The Phone is for
//! *initiating*: the app-side surface that opens exchanges.

use std::fmt;

use kamiroh_domain::actor::Address;
use kamiroh_domain::deadline::{DeadlineKind, Deadlines, FailureCause};
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::protocol::{TurnError, TurnProgress, TurnState};
use kamiroh_domain::vocabulary::{Ack, Message, Request, RequestId, Turn};
use kamiroh_ports::{Timer, Transport};

/// A live handle on one conversation: this actor ↔ `peer`.
#[derive(Debug)]
pub struct Phone<T: Transport, W: Timer> {
    self_address: Address,
    peer: Address,
    transport: T,
    timer: W,
    deadlines: Deadlines,
    state: TurnState,
    /// The delivery ack we are owed for a sent request half, and when our
    /// patience for it runs out.
    ack_due: Option<(RequestId, W::Instant)>,
    /// When our patience for the peer's next turn runs out.
    turn_due: Option<W::Instant>,
}

impl<T: Transport, W: Timer> Phone<T, W> {
    /// Open a conversation with `peer` — a local act; the wire is first
    /// touched by [`Phone::open`]. `deadlines` is this side's patience,
    /// mandatory and finite (decision 22).
    pub fn converse(
        self_address: Address,
        peer: Address,
        transport: T,
        deadlines: Deadlines,
        timer: W,
    ) -> Self {
        Self {
            self_address,
            peer,
            transport,
            timer,
            deadlines,
            state: TurnState::Idle,
            ack_due: None,
            turn_due: None,
        }
    }

    pub fn peer(&self) -> &Address {
        &self.peer
    }

    pub fn state(&self) -> TurnState {
        self.state
    }

    pub fn deadlines(&self) -> Deadlines {
        self.deadlines
    }

    /// Open an exchange: send the opening turn posing `request`.
    pub async fn open(&mut self, request: Request) -> Result<(), PhoneError> {
        self.send_turn(Turn::Open { request }).await.map(|_| ())
    }

    /// Open exchanges with several actors at ONE endpoint in a single
    /// send (`ARCHITECTURE.md`, decision 29), returning one ordinary
    /// Phone per conversation. Fan-out ends the moment this returns: the
    /// N conversations share nothing but their birth (and the one
    /// request id — which is what lets the single **batch receipt**
    /// settle every phone's ack wait: relay the receipt's `Ack` to
    /// *every* phone via [`Phone::note_ack`], exactly as the pump relays
    /// deaths and revocations; see `docs/EMBEDDING.md`).
    ///
    /// The receipt promises "the batch reached the endpoint" — less than
    /// the pairwise ack. A denied or absent sibling is therefore silence
    /// that surfaces at that phone's *turn* deadline, so fanned
    /// conversations have lost their short-deadline safety net: a
    /// tighter turn deadline in `deadlines` is the natural compensation.
    ///
    /// A refused batch send fails loudly at once (decision 26): the
    /// error returns and NO phones are created — nothing was armed, and
    /// a fresh call is legal immediately.
    pub async fn open_many(
        self_address: Address,
        endpoint: EndpointId,
        names: Vec<kamiroh_domain::actor::ActorName>,
        request: Request,
        transport: T,
        deadlines: Deadlines,
        timer: W,
    ) -> Result<Vec<Self>, PhoneError>
    where
        T: Clone,
        W: Clone,
    {
        let mut sender = transport.clone();
        sender
            .open_many(&self_address, &endpoint, &names, request.clone())
            .await
            .map_err(|e| PhoneError::Transport(e.to_string()))?;
        let now = timer.now();
        let mut phones = Vec::with_capacity(names.len());
        for name in names {
            let mut phone = Self::converse(
                self_address.clone(),
                Address::new(endpoint.clone(), name),
                transport.clone(),
                deadlines,
                timer.clone(),
            );
            // Walk the turn machine through the opening this batch just
            // performed on the wire — the same transition Phone::open
            // takes, so everything downstream is an ordinary conversation.
            phone
                .state
                .on_outgoing(&Turn::Open {
                    request: request.clone(),
                })
                .expect("an Open from Idle is always legal");
            phone.ack_due = Some((request.id, now + deadlines.ack));
            phone.turn_due = Some(now + deadlines.turn);
            phones.push(phone);
        }
        Ok(phones)
    }

    /// Send any turn, enforcing alternation. Returns whether the exchange
    /// continues or (on a `Close`) is concluded.
    ///
    /// On success the waits this turn creates are armed: an ack wait for its
    /// request half, and — unless this was the close — a wait for the peer's
    /// next turn. Both clocks start at the send.
    ///
    /// A **refused send fails the exchange at once** (decision 26): the
    /// transport's error is positive local evidence the turn never left, so
    /// rather than a wait that can never be answered, the exchange is dead
    /// (`FailureCause::SendFailed`), the error is returned, and a fresh
    /// [`Phone::open`] is legal immediately. One nuance: a refused `Close`
    /// leaves the state `Idle`, not `Failed` — the machine had already
    /// concluded the exchange before the send was attempted — so the truth
    /// there is "concluded, but the peer never received the final answer";
    /// the returned error is the caller's signal either way, and a fresh
    /// open is equally legal from both states.
    pub async fn send_turn(&mut self, turn: Turn) -> Result<TurnProgress, PhoneError> {
        let progress = self.state.on_outgoing(&turn)?;
        let posed = turn.request().map(|r| r.id);
        if let Err(e) = self
            .transport
            .send(&self.self_address, &self.peer, Message::Turn(turn))
            .await
        {
            self.state.fail();
            self.ack_due = None;
            self.turn_due = None;
            return Err(PhoneError::Transport(e.to_string()));
        }
        let now = self.timer.now();
        self.ack_due = posed.map(|id| (id, now + self.deadlines.ack));
        self.turn_due = match progress {
            TurnProgress::Continuing => Some(now + self.deadlines.turn),
            TurnProgress::Concluded => None,
        };
        Ok(progress)
    }

    /// Feed an incoming turn from this conversation's peer through the
    /// alternation rules. The caller (runtime or app pump) does this before
    /// treating the turn's content as part of the live exchange.
    ///
    /// A legal turn settles every pending wait: the peer's turn is what we
    /// were waiting for, and it supersedes a still-missing ack (the turn
    /// itself proves delivery happened).
    pub fn on_incoming(&mut self, turn: &Turn) -> Result<TurnProgress, PhoneError> {
        let progress = self.state.on_incoming(turn)?;
        self.ack_due = None;
        self.turn_due = None;
        Ok(progress)
    }

    /// Note a delivery ack from the peer's runtime. Returns whether it was
    /// the one we were waiting on (a stray or late ack changes nothing).
    pub fn note_ack(&mut self, ack: &Ack) -> bool {
        match self.ack_due {
            Some((id, _)) if id == ack.id => {
                self.ack_due = None;
                true
            }
            _ => false,
        }
    }

    /// The earliest pending deadline, if any wait is under way: what the
    /// pump should arm `Timer::sleep_until` with.
    pub fn next_due(&self) -> Option<(DeadlineKind, W::Instant)> {
        let ack = self.ack_due.map(|(_, at)| (DeadlineKind::Ack, at));
        let turn = self.turn_due.map(|at| (DeadlineKind::Turn, at));
        match (ack, turn) {
            (Some(a), Some(t)) => Some(if a.1 <= t.1 { a } else { t }),
            (a, t) => a.or(t),
        }
    }

    /// The pump's report that the armed deadline elapsed: the exchange is
    /// failed (decision 23) and every wait cleared. Returns the cause, or
    /// `None` if nothing was pending (a stale report; the exchange stands).
    ///
    /// After failure the conversation survives: [`Phone::open`] starts a
    /// fresh exchange; anything else is refused with
    /// [`TurnError::ExchangeFailed`].
    pub fn on_deadline_elapsed(&mut self) -> Option<FailureCause> {
        let (kind, _) = self.next_due()?;
        self.state.fail();
        self.ack_due = None;
        self.turn_due = None;
        Some(FailureCause::DeadlineElapsed(kind))
    }

    /// The pump's report that the transport declared an endpoint dead
    /// (decision 27). If it is this conversation's peer and an exchange is
    /// live, the exchange fails at once — positive evidence, no deadline
    /// waited out — and the cause is returned. The conversation survives:
    /// a fresh [`Phone::open`] may follow (the peer may come back).
    pub fn on_peer_death(&mut self, endpoint: &EndpointId) -> Option<FailureCause> {
        if self.peer.endpoint != *endpoint || !self.state.fail() {
            return None;
        }
        self.ack_due = None;
        self.turn_due = None;
        Some(FailureCause::Disconnected)
    }

    /// The pump's report that this side's operator revoked an endpoint
    /// (decision 28). The Phone guards its own inbound traffic with the
    /// application's allowlist, which the runtime cannot reach — so the
    /// application that revokes there must tell its Phones, exactly as it
    /// relays deaths. If the revoked endpoint is this conversation's peer
    /// and an exchange is live, the exchange fails at once — positive
    /// evidence, no deadline waited out — and the cause is returned. The
    /// conversation survives: a fresh [`Phone::open`] is legal after a
    /// re-admission (but see [`FailureCause::Revoked`] — do not reopen
    /// unprompted against a peer this side deliberately cut off).
    pub fn on_revoked(&mut self, endpoint: &EndpointId) -> Option<FailureCause> {
        if self.peer.endpoint != *endpoint || !self.state.fail() {
            return None;
        }
        self.ack_due = None;
        self.turn_due = None;
        Some(FailureCause::Revoked)
    }

    /// This side's timer — for pumps that arm the wait themselves.
    pub fn timer(&self) -> &W {
        &self.timer
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhoneError {
    /// The turn violates alternation, answers the wrong request, or belongs
    /// to a failed exchange.
    Turn(TurnError),
    /// The transport refused the send.
    Transport(String),
}

impl From<TurnError> for PhoneError {
    fn from(e: TurnError) -> Self {
        PhoneError::Turn(e)
    }
}

impl fmt::Display for PhoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhoneError::Turn(e) => write!(f, "turn refused: {e}"),
            PhoneError::Transport(e) => write!(f, "transport failed: {e}"),
        }
    }
}

impl std::error::Error for PhoneError {}
