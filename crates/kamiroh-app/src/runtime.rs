//! A deliberately small, single-endpoint actor runtime.
//!
//! This is the toy that `kamiroh-adapter-kameo` replaces: it owns the
//! endpoint's local actors, binds them through the [`Registry`] port, routes
//! each delivery through [`inbound::process`](crate::inbound::process), and
//! executes harness commands. Its value is fixing the *shape* — what owning
//! actors and routing deliveries means — against the memory transport, so the
//! Kameo adapter later swaps the engine, not the design.
//!
//! The runtime holds the responder side of the deadline design (decisions
//! 22–25): every installed actor carries a mandatory [`Deadlines`]; while a
//! conversation waits — for a delivery ack of a sent request half, for the
//! peer's next turn — [`LocalRuntime::step`] races the earliest due instant
//! against the inbox, and an elapsed deadline fails the exchange loudly: the
//! turn machine moves to `Failed`, the party is told, and an
//! [`Event::ExchangeFailed`] reaches the observer. Denied deliveries and
//! dropped illegal turns reach the observer too — never the wire.

use std::collections::{HashMap, HashSet};
use std::fmt;

use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::allowlist::Allowlist;
use kamiroh_domain::deadline::{DeadlineKind, Deadlines, FailureCause};
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::protocol::{TurnProgress, TurnState};
use kamiroh_domain::vocabulary::{Harness, Message, RequestId};
use kamiroh_ports::{
    DeathWatch, DynDeathWatch, DynParty, Event, Inbox, Observer, Registry, Timer, Transport,
};

use crate::inbound::{Inbound, process};
use crate::parties::EchoParty;
use crate::wait::{Waited, next_or_deadline, next_or_deadline_or_death};

/// What kind of party sits behind a local actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// Interprets harness commands. Privileged (`ARCHITECTURE.md`, decision 6).
    Harness,
    /// An ordinary actor: receives requests, acks them. In v0 no party is
    /// wired behind it yet — the ack (delivery, not answer) is the point.
    Plain,
}

/// One conversation's responder-side protocol state: the turn machine plus
/// the waits currently armed against this actor's deadlines.
struct PeerState<I> {
    state: TurnState,
    /// The delivery ack owed for a request half this actor sent, and when
    /// patience for it runs out.
    ack_due: Option<(RequestId, I)>,
    /// When patience for the peer's next turn runs out.
    turn_due: Option<I>,
}

impl<I> Default for PeerState<I> {
    fn default() -> Self {
        Self {
            state: TurnState::default(),
            ack_due: None,
            turn_due: None,
        }
    }
}

struct LocalActor<Ib, I> {
    inbox: Ib,
    allowlist: Allowlist,
    kind: ActorKind,
    /// This actor's patience (decision 22): mandatory, applying to every
    /// conversation it responds in.
    deadlines: Deadlines,
    /// The party behind this actor, if one is wired (decision 16).
    party: Option<Box<dyn DynParty>>,
    /// Per-conversation protocol state, keyed by peer (decision 17).
    conversations: HashMap<Address, PeerState<I>>,
}

/// What one [`LocalRuntime::step`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stepped {
    /// A delivery was taken and acted on (possibly dropped: denied or
    /// illegal — see the observer for which).
    Delivered,
    /// A deadline elapsed first: the exchange with `peer` failed.
    ExchangeFailed { peer: Address, cause: FailureCause },
    /// The transport reported `endpoint` dead (decision 27): this actor's
    /// live exchanges with peers there failed — `exchanges_failed` of them
    /// (possibly zero, when nothing was in flight). The conversations
    /// survive; the endpoint stays remembered as dead until it is heard
    /// from again.
    PeerVanished {
        endpoint: EndpointId,
        exchanges_failed: usize,
    },
}

/// The toy runtime for one endpoint.
pub struct LocalRuntime<T: Transport, R: Registry, W: Timer> {
    endpoint: EndpointId,
    transport: T,
    registry: R,
    timer: W,
    /// Patience given to harness-spawned actors (decision 22: deadlines are
    /// mandatory, so the operator of a harness endpoint chooses them for
    /// everything it spawns).
    spawn_deadlines: Deadlines,
    observer: Option<Box<dyn Observer>>,
    /// The transport's death evidence, if watched (decision 27).
    death_watch: Option<Box<dyn DynDeathWatch>>,
    /// Endpoints reported dead and not heard from since. Consulted at every
    /// step so each actor's affected exchanges fail as it runs; cleared the
    /// moment a delivery from the endpoint proves it alive again.
    dead: HashSet<EndpointId>,
    actors: HashMap<ActorName, LocalActor<R::Inbox, W::Instant>>,
}

impl<T: Transport, R: Registry, W: Timer> LocalRuntime<T, R, W> {
    pub fn new(
        endpoint: EndpointId,
        transport: T,
        registry: R,
        timer: W,
        spawn_deadlines: Deadlines,
    ) -> Self {
        Self {
            endpoint,
            transport,
            registry,
            timer,
            spawn_deadlines,
            observer: None,
            death_watch: None,
            dead: HashSet::new(),
            actors: HashMap::new(),
        }
    }

    pub fn endpoint(&self) -> &EndpointId {
        &self.endpoint
    }

    /// Route local operational [`Event`]s (decision 25) to `observer`.
    pub fn set_observer(&mut self, observer: impl Observer + 'static) {
        self.observer = Some(Box::new(observer));
    }

    /// Race the transport's death evidence (decision 27) into every
    /// [`LocalRuntime::step`]: a reported death fails the affected live
    /// exchanges at once, well before any deadline.
    pub fn watch_deaths(&mut self, watch: impl DeathWatch + Send + 'static) {
        self.death_watch = Some(Box::new(watch));
    }

    fn emit(&mut self, event: Event) {
        if let Some(observer) = &mut self.observer {
            observer.on_event(event);
        }
    }

    /// Install an actor: bind its address via the [`Registry`] port and hold
    /// its inbox. Dropping the actor (see harness `Stop`) unbinds it.
    pub fn install(
        &mut self,
        name: ActorName,
        allowlist: Allowlist,
        deadlines: Deadlines,
        kind: ActorKind,
    ) -> Result<(), RuntimeError> {
        self.install_inner(name, allowlist, deadlines, kind, None)
    }

    /// Install an actor with the party behind it (decision 16): incoming
    /// turns are handed to `party`, its returned turns sent for it.
    pub fn install_party(
        &mut self,
        name: ActorName,
        allowlist: Allowlist,
        deadlines: Deadlines,
        party: Box<dyn DynParty>,
    ) -> Result<(), RuntimeError> {
        self.install_inner(name, allowlist, deadlines, ActorKind::Plain, Some(party))
    }

    fn install_inner(
        &mut self,
        name: ActorName,
        allowlist: Allowlist,
        deadlines: Deadlines,
        kind: ActorKind,
        party: Option<Box<dyn DynParty>>,
    ) -> Result<(), RuntimeError> {
        if self.actors.contains_key(&name) {
            return Err(RuntimeError::NameInUse);
        }
        let address = Address::new(self.endpoint.clone(), name.clone());
        let inbox = self
            .registry
            .bind(&address)
            .map_err(|e| RuntimeError::Bind(e.to_string()))?;
        self.actors.insert(
            name,
            LocalActor {
                inbox,
                allowlist,
                kind,
                deadlines,
                party,
                conversations: HashMap::new(),
            },
        );
        Ok(())
    }

    /// Admit `endpoint` on a *running* actor's allowlist (decision 28). Takes
    /// effect on the next delivery — but does not replay anything: deliveries
    /// denied before the admission were dropped, not queued, so the peer must
    /// send again. Idempotent (R5).
    pub fn admit(&mut self, name: &ActorName, endpoint: EndpointId) -> Result<(), RuntimeError> {
        let actor = self
            .actors
            .get_mut(name)
            .ok_or(RuntimeError::UnknownActor)?;
        actor.allowlist.admit(endpoint);
        Ok(())
    }

    /// Revoke `endpoint` from a *running* actor's allowlist (decision 28).
    /// Bites on the very next delivery, even mid-conversation — and, because
    /// a revocation is positive local evidence that nothing further will be
    /// heard from that endpoint, the actor's **live** exchanges with peers
    /// there fail at once (`FailureCause::Revoked`, decision 26's principle),
    /// rather than waiting out deadlines this side has already answered.
    /// Conversations survive: a fresh exchange is legal on re-admission.
    /// Returns how many exchanges were failed. Idempotent at every layer
    /// (R5): already-failed and idle conversations are left untouched, so a
    /// second revocation fails nothing and emits nothing. Never touches
    /// connections (R4) — policy is the actor's, the pipe is the node's.
    pub async fn revoke(
        &mut self,
        name: &ActorName,
        endpoint: &EndpointId,
    ) -> Result<usize, RuntimeError> {
        let affected: Vec<Address> = {
            let actor = self
                .actors
                .get_mut(name)
                .ok_or(RuntimeError::UnknownActor)?;
            actor.allowlist.revoke(endpoint);
            actor
                .conversations
                .iter()
                .filter(|(peer, ps)| {
                    matches!(
                        ps.state,
                        TurnState::AwaitingTheirTurn { .. } | TurnState::OweThem { .. }
                    ) && peer.endpoint == *endpoint
                })
                .map(|(peer, _)| peer.clone())
                .collect()
        };
        // No tombstones here, deliberately: deaths need the `dead` set
        // because they arrive asynchronously during a step's race, while a
        // revocation happens strictly between steps (`step` holds `&mut
        // self` across its await). Fail directly.
        for peer in &affected {
            self.fail_exchange(name, peer, FailureCause::Revoked)
                .await?;
        }
        Ok(affected.len())
    }

    /// Fail one conversation's exchange (decision 23) and be loud about it
    /// (decision 25): the machine moves to `Failed`, every wait clears, the
    /// party is told, and the observer sees the event. One home for the
    /// path shared by elapsed deadlines, refused sends, and illegal replies.
    ///
    /// Known wrinkle (deliberate): when a *`Close`* send is refused, the
    /// machine has already concluded (`Idle`), so `fail()` no-ops and the
    /// event/notification say "failed" while the state says "idle" — the
    /// party is still told, honestly, that the peer never got its final
    /// answer; a later stray turn falls to `NoExchange` rather than
    /// `ExchangeFailed`. Both refuse it, which is what matters.
    async fn fail_exchange(
        &mut self,
        name: &ActorName,
        peer: &Address,
        cause: FailureCause,
    ) -> Result<Stepped, RuntimeError> {
        let local = Address::new(self.endpoint.clone(), name.clone());
        {
            let actor = self
                .actors
                .get_mut(name)
                .ok_or(RuntimeError::UnknownActor)?;
            if let Some(ps) = actor.conversations.get_mut(peer) {
                ps.state.fail();
                ps.ack_due = None;
                ps.turn_due = None;
            }
            if let Some(party) = &mut actor.party {
                party.on_exchange_failed_boxed(peer, cause).await;
            }
        }
        self.emit(Event::ExchangeFailed {
            local,
            peer: peer.clone(),
            cause,
        });
        Ok(Stepped::ExchangeFailed {
            peer: peer.clone(),
            cause,
        })
    }

    /// Fail this actor's live exchanges with peers at one dead endpoint
    /// (decision 27). Returns `None` when no tombstoned endpoint touches a
    /// live exchange of this actor.
    async fn sweep_dead(&mut self, name: &ActorName) -> Result<Option<Stepped>, RuntimeError> {
        let affected: Vec<Address> = {
            let actor = self
                .actors
                .get_mut(name)
                .ok_or(RuntimeError::UnknownActor)?;
            let dead = &self.dead;
            actor
                .conversations
                .iter()
                .filter(|(peer, ps)| {
                    matches!(
                        ps.state,
                        TurnState::AwaitingTheirTurn { .. } | TurnState::OweThem { .. }
                    ) && dead.contains(&peer.endpoint)
                })
                .map(|(peer, _)| peer.clone())
                .collect()
        };
        let Some(first) = affected.first() else {
            return Ok(None);
        };
        let endpoint = first.endpoint.clone();
        let peers: Vec<Address> = affected
            .into_iter()
            .filter(|a| a.endpoint == endpoint)
            .collect();
        let mut failed = 0;
        for peer in &peers {
            self.fail_exchange(name, peer, FailureCause::Disconnected)
                .await?;
            failed += 1;
        }
        Ok(Some(Stepped::PeerVanished {
            endpoint,
            exchanges_failed: failed,
        }))
    }

    /// The earliest armed deadline across this actor's conversations.
    fn earliest_due(
        actor: &LocalActor<R::Inbox, W::Instant>,
    ) -> Option<(Address, DeadlineKind, W::Instant)> {
        let mut earliest: Option<(Address, DeadlineKind, W::Instant)> = None;
        for (peer, ps) in &actor.conversations {
            let candidates = [
                ps.ack_due.map(|(_, at)| (DeadlineKind::Ack, at)),
                ps.turn_due.map(|at| (DeadlineKind::Turn, at)),
            ];
            for (kind, at) in candidates.into_iter().flatten() {
                if earliest.as_ref().is_none_or(|(_, _, e)| at < *e) {
                    earliest = Some((peer.clone(), kind, at));
                }
            }
        }
        earliest
    }

    /// Wait for `name`'s actor's next event and act on it: a delivery
    /// (enforce admission, ack admitted requests, execute harness commands,
    /// drive the party) or an elapsed deadline (fail the exchange loudly).
    /// One event per call, so tests stay deterministic — and an
    /// already-elapsed deadline outranks an already-queued delivery.
    pub async fn step(&mut self, name: &ActorName) -> Result<Stepped, RuntimeError> {
        // Deaths already known outrank everything (decision 27): fail this
        // actor's affected exchanges before waiting for anything new.
        if let Some(stepped) = self.sweep_dead(name).await? {
            return Ok(stepped);
        }

        let (allowlist, kind, due) = {
            let actor = self
                .actors
                .get_mut(name)
                .ok_or(RuntimeError::UnknownActor)?;
            (
                actor.allowlist.clone(),
                actor.kind,
                Self::earliest_due(actor),
            )
        };

        let waited = {
            let timer = &self.timer;
            let death_watch = &mut self.death_watch;
            let actor = self
                .actors
                .get_mut(name)
                .ok_or(RuntimeError::UnknownActor)?;
            let sleep = due.as_ref().map(|(_, _, at)| timer.sleep_until(*at));
            match death_watch {
                Some(watch) => {
                    next_or_deadline_or_death(&mut actor.inbox, sleep, watch.next_death_boxed())
                        .await
                }
                None => match sleep {
                    Some(sleep) => next_or_deadline(&mut actor.inbox, sleep).await,
                    None => Waited::Delivery(actor.inbox.next().await),
                },
            }
        };

        let delivery = match waited {
            Waited::PeerDeath(endpoint) => {
                // Positive evidence: remember it, and fail what it touches.
                self.dead.insert(endpoint.clone());
                return match self.sweep_dead(name).await? {
                    Some(stepped) => Ok(stepped),
                    // Nothing of this actor's was in flight; the tombstone
                    // stands for other actors and later exchanges.
                    None => Ok(Stepped::PeerVanished {
                        endpoint,
                        exchanges_failed: 0,
                    }),
                };
            }
            Waited::DeadlineElapsed => {
                let (peer, kind, _) = due.expect("deadline cannot elapse unarmed");
                return self
                    .fail_exchange(name, &peer, FailureCause::DeadlineElapsed(kind))
                    .await;
            }
            Waited::Delivery(None) => return Err(RuntimeError::InboxClosed),
            Waited::Delivery(Some(delivery)) => delivery,
        };

        let self_address = delivery.to.clone();
        let origin = delivery.from.clone();
        // Hearing from an endpoint proves it alive: clear its tombstone.
        self.dead.remove(&origin.endpoint);
        match process(&allowlist, delivery) {
            Inbound::Denied => {
                self.emit(Event::DeliveryDenied {
                    from: origin,
                    to: self_address,
                });
                Ok(Stepped::Delivered)
            }
            Inbound::Request { reply_to, ack, .. } => {
                // In v0 there is no party wired behind a Plain actor yet; the
                // request is received and the delivery acknowledged.
                self.send(&self_address, &reply_to, ack).await?;
                Ok(Stepped::Delivered)
            }
            Inbound::AckReceived(ack) => {
                // The receipt we may have been waiting on: settle the wait.
                let actor = self
                    .actors
                    .get_mut(name)
                    .ok_or(RuntimeError::UnknownActor)?;
                if let Some(ps) = actor.conversations.get_mut(&origin)
                    && matches!(ps.ack_due, Some((id, _)) if id == ack.id)
                {
                    ps.ack_due = None;
                }
                Ok(Stepped::Delivered)
            }
            Inbound::Harness { harness, reply_to } => {
                if kind != ActorKind::Harness {
                    let reply = Message::Harness(Harness::Failed {
                        reason: "not a harness actor".into(),
                    });
                    self.send(&self_address, &reply_to, reply).await?;
                    return Ok(Stepped::Delivered);
                }
                let reply = self.execute(harness, &reply_to);
                if let Some(reply) = reply {
                    self.send(&self_address, &reply_to, reply).await?;
                }
                Ok(Stepped::Delivered)
            }
            Inbound::Turn {
                turn,
                for_actor: _,
                reply_to,
                ack,
            } => {
                // Validate against this conversation's alternation state;
                // illegal turns are dropped — silently on the wire,
                // observably at home (decision 25).
                let legal = {
                    let actor = self
                        .actors
                        .get_mut(name)
                        .ok_or(RuntimeError::UnknownActor)?;
                    let ps = actor.conversations.entry(reply_to.clone()).or_default();
                    match ps.state.on_incoming(&turn) {
                        Ok(_) => {
                            // Their turn is what we were waiting for — and it
                            // supersedes a still-missing ack (the turn itself
                            // proves delivery happened).
                            ps.ack_due = None;
                            ps.turn_due = None;
                            None
                        }
                        Err(error) => Some(error),
                    }
                };
                if let Some(error) = legal {
                    self.emit(Event::IllegalTurnDropped {
                        from: reply_to,
                        to: self_address,
                        error,
                    });
                    return Ok(Stepped::Delivered);
                }
                // Ack on handover — the fast receipt, before the party
                // thinks. A refused ack send is deliberately IGNORED
                // (Casey's ruling on the 2026-08-23 external review, and
                // KameoRuntime's behavior on the same path): the arriving
                // turn already proved delivery to the peer's benefit — a
                // turn supersedes a missing ack on its side — a transient
                // blip then recovers gracefully, and a genuinely broken
                // transport still fails LOUDLY seconds later when the
                // reply send goes through fail_exchange (decision 26).
                // The previous `?` here aborted the step after the machine
                // had already moved, leaving the exchange un-deadlined
                // with the party never called — the silent wedge this
                // spike exists to abolish, in the runtime documented as
                // the readable reference.
                if let Some(ack) = ack {
                    let _ = self.send(&self_address, &reply_to, ack).await;
                }
                // Hand the turn to the party; its state change completes
                // before its reply exists, let alone is sent.
                let reply = {
                    let actor = self
                        .actors
                        .get_mut(name)
                        .ok_or(RuntimeError::UnknownActor)?;
                    match &mut actor.party {
                        Some(party) => party.on_turn_boxed(&reply_to, turn).await,
                        None => None,
                    }
                };
                if let Some(reply_turn) = reply {
                    let now = self.timer.now();
                    let sendable = {
                        let actor = self
                            .actors
                            .get_mut(name)
                            .ok_or(RuntimeError::UnknownActor)?;
                        let deadlines = actor.deadlines;
                        let ps = actor.conversations.entry(reply_to.clone()).or_default();
                        match ps.state.on_outgoing(&reply_turn) {
                            Ok(progress) => {
                                // Arm the waits this reply creates: the ack of
                                // its request half, and (unless it closed the
                                // exchange) the peer's next turn.
                                ps.ack_due =
                                    reply_turn.request().map(|r| (r.id, now + deadlines.ack));
                                ps.turn_due = match progress {
                                    TurnProgress::Continuing => Some(now + deadlines.turn),
                                    TurnProgress::Concluded => None,
                                };
                                true
                            }
                            Err(_) => false,
                        }
                    };
                    if !sendable {
                        // The party's own reply was illegal: dropped, and the
                        // exchange fails loudly rather than lingering with no
                        // deadline armed (decision 26).
                        return self
                            .fail_exchange(name, &reply_to, FailureCause::IllegalReply)
                            .await;
                    }
                    if self
                        .send(&self_address, &reply_to, Message::Turn(reply_turn))
                        .await
                        .is_err()
                    {
                        // The transport refused the reply: positive local
                        // evidence it never left, so the exchange fails at
                        // once (decision 26) instead of waiting out the
                        // waits armed above.
                        return self
                            .fail_exchange(name, &reply_to, FailureCause::SendFailed)
                            .await;
                    }
                }
                Ok(Stepped::Delivered)
            }
        }
    }

    /// Execute a harness command, returning the reply to send. Reply kinds
    /// (`Spawned`, `Stopped`, `Pong`, `Failed`) arriving here are ignored.
    fn execute(&mut self, command: Harness, controller: &Address) -> Option<Message> {
        let reply = match command {
            Harness::Ping => Harness::Pong,
            Harness::Spawn { name } => {
                // The spawned actor admits the controlling endpoint only,
                // and gets an EchoParty behind it — the first real Party.
                let mut allowlist = Allowlist::empty();
                allowlist.admit(controller.endpoint.clone());
                match self.install_party(
                    name.clone(),
                    allowlist,
                    self.spawn_deadlines,
                    Box::new(EchoParty::new()),
                ) {
                    Ok(()) => Harness::Spawned { name },
                    Err(e) => Harness::Failed {
                        reason: e.to_string(),
                    },
                }
            }
            Harness::Stop { name } => {
                // Dropping the actor drops its inbox, which unbinds the
                // address at the transport (Registry contract).
                match self.actors.remove(&name) {
                    Some(_) => Harness::Stopped { name },
                    None => Harness::Failed {
                        reason: "no such actor".into(),
                    },
                }
            }
            Harness::Spawned { .. }
            | Harness::Stopped { .. }
            | Harness::Pong
            | Harness::Failed { .. } => return None,
        };
        Some(Message::Harness(reply))
    }

    async fn send(
        &mut self,
        from: &Address,
        to: &Address,
        message: Message,
    ) -> Result<(), RuntimeError> {
        self.transport
            .send(from, to, message)
            .await
            .map_err(|e| RuntimeError::Transport(e.to_string()))
    }
}

/// Spike-pragmatic error type: adapter errors are carried as text rather than
/// generics, keeping the runtime's signature simple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    UnknownActor,
    NameInUse,
    InboxClosed,
    Bind(String),
    Transport(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::UnknownActor => f.write_str("no such actor in this runtime"),
            RuntimeError::NameInUse => f.write_str("an actor with this name is already installed"),
            RuntimeError::InboxClosed => f.write_str("the actor's inbox is closed"),
            RuntimeError::Bind(e) => write!(f, "binding failed: {e}"),
            RuntimeError::Transport(e) => write!(f, "transport failed: {e}"),
        }
    }
}

impl std::error::Error for RuntimeError {}
