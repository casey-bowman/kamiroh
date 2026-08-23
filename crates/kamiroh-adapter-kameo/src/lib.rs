//! Kameo runtime adapter.
//!
//! Animates domain actors as Kameo actors — the engine-for-engine replacement
//! for [`kamiroh_app::runtime::LocalRuntime`]. This is a *driving* adapter
//! (`ARCHITECTURE.md`, decision 13): it hosts the app layer's behavior — one
//! Kameo actor per domain actor, each fed by a pump task draining its
//! transport [`Inbox`], each delivery routed through
//! [`inbound::process`](kamiroh_app::inbound::process).
//!
//! The toy `LocalRuntime` stays in the tree as the reference implementation;
//! this adapter reproduces its observable behavior with real concurrency —
//! actors run autonomously, no manual `step()`.
//!
//! ## A historical note on the "assumption point" comments
//!
//! This adapter was first drafted in spike 1 against `kameo = "0.17"`-era
//! APIs **without compiling** (the cloud sandbox could not reach
//! crates.io then, and blind-writing with assumption lists was the
//! workflow — since retired, see `docs/WORKFLOW.md`). It has long since
//! been compiled, tested, reviewed, and extended against the real
//! dependency (`kameo 0.22` at spike 2's close). The numbered
//! "assumption point" markers below survive as archaeology: they record
//! which guesses the first local build pass had to correct, and are no
//! longer live uncertainty.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kameo::actor::{ActorRef, Spawn};
use kameo::message::{Context, Message};

use kamiroh_app::inbound::{Inbound, process};
use kamiroh_app::parties::EchoParty;
use kamiroh_app::runtime::{ActorKind, RuntimeError};
use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::allowlist::Allowlist;
use kamiroh_domain::deadline::{DeadlineKind, Deadlines, FailureCause};
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::protocol::{TurnProgress, TurnState};
use kamiroh_domain::vocabulary::{Harness, Message as Vocab, RequestId};
use kamiroh_ports::{DeathWatch, DynParty, Event, Inbox, Observer, Registry, Timer, Transport};

/// The Kameo-backed runtime for one endpoint. Cheap to clone; clones share
/// the roster.
///
/// Must be used inside a tokio runtime (pump tasks are `tokio::spawn`ed).
pub struct KameoRuntime<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    inner: Arc<Inner<T, R>>,
}

impl<T, R> Clone for KameoRuntime<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    endpoint: EndpointId,
    /// Template transport handle, cloned into each hosted actor.
    transport: T,
    registry: Mutex<R>,
    roster: Mutex<HashMap<ActorName, Entry<T, R>>>,
    /// Patience given to harness-spawned actors (decision 22).
    spawn_deadlines: Deadlines,
    /// Local operational events (decision 25) go here, if set.
    observer: Mutex<Option<Box<dyn Observer>>>,
}

struct Entry<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    actor_ref: ActorRef<Host<T, R>>,
    pump: tokio::task::JoinHandle<()>,
}

impl<T, R> KameoRuntime<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    /// `spawn_deadlines` is the patience given to every harness-spawned
    /// actor: deadlines are mandatory (decision 22), and the operator of a
    /// harness endpoint chooses them for everything it spawns.
    pub fn new(
        endpoint: EndpointId,
        transport: T,
        registry: R,
        spawn_deadlines: Deadlines,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                endpoint,
                transport,
                registry: Mutex::new(registry),
                roster: Mutex::new(HashMap::new()),
                spawn_deadlines,
                observer: Mutex::new(None),
            }),
        }
    }

    pub fn endpoint(&self) -> &EndpointId {
        &self.inner.endpoint
    }

    /// Route local operational [`Event`]s (decision 25) to `observer`.
    ///
    /// The observer is called with the runtime's internal lock held, so no
    /// event is ever lost to a concurrent emit — in exchange, `observer`
    /// must **not** call back into this runtime from inside
    /// [`Observer::on_event`] (no `set_observer`, `install`, `stop`,
    /// `admit`, or `revoke` — in particular, an observer that revokes on
    /// [`Event::DeliveryDenied`] is an auto-ban that deadlocks);
    /// doing so deadlocks. Observe, record, return.
    pub fn set_observer(&self, observer: impl Observer + 'static) {
        *self.inner.observer.lock().expect("observer poisoned") = Some(Box::new(observer));
    }

    fn emit(&self, event: Event) {
        // Events are delivered one at a time, under the lock, so concurrent
        // emits from independent hosts serialize and NOTHING IS EVER LOST —
        // decision 25's whole point is that these events are reliably
        // assertable. The cost is a documented rule (decision 26): observer
        // code must not call back into this runtime from inside `on_event`
        // (e.g. `set_observer`, `install`, `stop`, `admit`, `revoke`), or it deadlocks on this
        // non-reentrant mutex — loudly, which beats silently losing events.
        // (A previous take-the-observer-out form allowed re-entry but
        // dropped concurrent events; see the review reproducer in
        // tests/observer_concurrency.rs.)
        if let Some(observer) = self
            .inner
            .observer
            .lock()
            .expect("observer poisoned")
            .as_mut()
        {
            observer.on_event(event);
        }
    }

    /// Install an actor: bind its address (Registry port), spawn its Kameo
    /// host, and start the pump task feeding deliveries from the transport
    /// inbox into the host's mailbox.
    pub fn install(
        &self,
        name: ActorName,
        allowlist: Allowlist,
        deadlines: Deadlines,
        kind: ActorKind,
    ) -> Result<(), RuntimeError> {
        self.install_inner(name, allowlist, deadlines, kind, None)
    }

    /// Install an actor with the party behind it (decision 16).
    pub fn install_party(
        &self,
        name: ActorName,
        allowlist: Allowlist,
        deadlines: Deadlines,
        party: Box<dyn DynParty>,
    ) -> Result<(), RuntimeError> {
        self.install_inner(name, allowlist, deadlines, ActorKind::Plain, Some(party))
    }

    fn install_inner(
        &self,
        name: ActorName,
        allowlist: Allowlist,
        deadlines: Deadlines,
        kind: ActorKind,
        party: Option<Box<dyn DynParty>>,
    ) -> Result<(), RuntimeError> {
        let mut roster = self.inner.roster.lock().expect("roster poisoned");
        if roster.contains_key(&name) {
            return Err(RuntimeError::NameInUse);
        }
        let address = Address::new(self.inner.endpoint.clone(), name.clone());
        let mut inbox = self
            .inner
            .registry
            .lock()
            .expect("registry poisoned")
            .bind(&address)
            .map_err(|e| RuntimeError::Bind(e.to_string()))?;

        let host = Host {
            address,
            allowlist,
            kind,
            deadlines,
            transport: self.inner.transport.clone(),
            runtime: self.clone(),
            party,
            conversations: HashMap::new(),
        };
        let actor_ref = Host::spawn(host);

        let pump_ref = actor_ref.clone();
        let pump = tokio::spawn(async move {
            // The pump owns the transport inbox; when this task ends (or is
            // aborted by `stop`), the inbox drops, which unbinds the address
            // at the transport (Registry contract, decision 12).
            while let Some(delivery) = inbox.next().await {
                if pump_ref.tell(Deliver(delivery)).await.is_err() {
                    break; // host stopped
                }
            }
        });

        roster.insert(name, Entry { actor_ref, pump });
        Ok(())
    }

    /// Race the transport's death evidence (decision 27) into this
    /// runtime: every reported death is fanned out to every hosted actor,
    /// which fails its live exchanges with peers at that endpoint at once —
    /// positive evidence, no deadline waited out. `abort()` the returned
    /// handle to end the watch; **dropping it detaches** the task (tokio
    /// semantics), which keeps watching for the runtime's lifetime.
    pub fn watch_deaths(
        &self,
        mut watch: impl DeathWatch + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        let runtime = self.clone();
        tokio::spawn(async move {
            while let Some(endpoint) = watch.next_death().await {
                let refs: Vec<_> = runtime
                    .inner
                    .roster
                    .lock()
                    .expect("roster poisoned")
                    .values()
                    .map(|entry| entry.actor_ref.clone())
                    .collect();
                for actor_ref in refs {
                    let _ = actor_ref
                        .tell(PeerDied {
                            endpoint: endpoint.clone(),
                        })
                        .await;
                }
            }
        })
    }

    /// One hosted actor's `ActorRef`, cloned out from under the roster lock
    /// — the guard must drop before any `.await`, both for `Send`-ness and
    /// because the host's own handlers (`Host::execute`) take this lock.
    fn actor_ref(&self, name: &ActorName) -> Result<ActorRef<Host<T, R>>, RuntimeError> {
        let roster = self.inner.roster.lock().expect("roster poisoned");
        roster
            .get(name)
            .map(|entry| entry.actor_ref.clone())
            .ok_or(RuntimeError::UnknownActor)
    }

    /// Admit `endpoint` on a *running* actor's allowlist (decision 28).
    /// Takes effect on deliveries processed after this call resolves — but
    /// replays nothing: deliveries denied before the admission were dropped,
    /// not queued, so the peer must send again. Idempotent (R5).
    ///
    /// `ask`-based, so awaiting this call is a real synchronization point: a
    /// test (or operator) that awaits it may rely on the admission being in
    /// force for whatever it sends next.
    pub async fn admit(&self, name: &ActorName, endpoint: EndpointId) -> Result<(), RuntimeError> {
        let actor_ref = self.actor_ref(name)?;
        actor_ref
            .ask(Admit { endpoint })
            .await
            .map_err(|_| RuntimeError::UnknownActor)
    }

    /// Revoke `endpoint` from a *running* actor's allowlist (decision 28).
    /// Bites on every delivery processed after this call resolves — note the
    /// honest limit: deliveries *already queued* in the host's mailbox ahead
    /// of the revoke are processed first, under the admission that held when
    /// they are handled, and revocation latency is bounded by the current
    /// handler completing (decision 26's processing-time shape). A
    /// revocation is positive local evidence, so the actor's **live**
    /// exchanges with peers at that endpoint fail at once
    /// (`FailureCause::Revoked`); conversations survive, and a fresh
    /// exchange is legal on re-admission. Returns how many exchanges were
    /// failed — idempotent at every layer (R5): a second revocation fails
    /// nothing and emits nothing. Never touches connections (R4).
    pub async fn revoke(
        &self,
        name: &ActorName,
        endpoint: &EndpointId,
    ) -> Result<usize, RuntimeError> {
        let actor_ref = self.actor_ref(name)?;
        actor_ref
            .ask(Revoke {
                endpoint: endpoint.clone(),
            })
            .await
            .map_err(|_| RuntimeError::UnknownActor)
    }

    /// Stop an actor: end its pump (unbinding its address) and stop its host.
    pub fn stop(&self, name: &ActorName) -> Result<(), RuntimeError> {
        let entry = {
            let mut roster = self.inner.roster.lock().expect("roster poisoned");
            roster.remove(name).ok_or(RuntimeError::UnknownActor)?
        };
        entry.pump.abort();
        entry.actor_ref.kill();
        Ok(())
    }
}

/// The Kameo actor hosting one domain actor's behavior.
struct Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    address: Address,
    allowlist: Allowlist,
    kind: ActorKind,
    /// This actor's patience (decision 22).
    deadlines: Deadlines,
    transport: T,
    runtime: KameoRuntime<T, R>,
    /// The party behind this actor, if one is wired (decision 16).
    party: Option<Box<dyn DynParty>>,
    /// Per-conversation protocol state and armed waits, keyed by peer
    /// (decisions 17, 22).
    conversations: HashMap<Address, PeerWaits>,
}

/// One conversation's responder-side state in the Kameo host: the turn
/// machine plus the timer tasks currently armed against the actor's
/// deadlines. Each timer task sleeps (real tokio time) and then tells the
/// host [`DeadlineElapsed`]; settling a wait aborts its task. An aborted
/// task's already-queued message is recognized as stale by re-checking the
/// wait it claims to expire.
#[derive(Default)]
struct PeerWaits {
    state: TurnState,
    ack: Option<(RequestId, tokio::task::JoinHandle<()>)>,
    turn: Option<tokio::task::JoinHandle<()>>,
    /// Bumped every time this conversation arms a round of waits. Carried in
    /// [`DeadlineElapsed`] so a report from a *superseded* timer task — one
    /// aborted after its message had already queued — is recognized as stale
    /// even though a wait of the same kind is armed again.
    epoch: u64,
}

impl PeerWaits {
    fn settle_ack(&mut self, id: RequestId) {
        if let Some((due, handle)) = &self.ack
            && *due == id
        {
            handle.abort();
            self.ack = None;
        }
    }

    fn settle_all(&mut self) {
        if let Some((_, handle)) = self.ack.take() {
            handle.abort();
        }
        if let Some(handle) = self.turn.take() {
            handle.abort();
        }
    }
}

impl<T, R> kameo::Actor for Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    // Assumption point 1: adjust to the actual kameo Actor trait shape.
    type Args = Self;
    type Error = std::convert::Infallible;

    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(args)
    }

    async fn on_stop(
        &mut self,
        _actor_ref: kameo::actor::WeakActorRef<Self>,
        _reason: kameo::error::ActorStopReason,
    ) -> Result<(), Self::Error> {
        // Dropping a tokio `JoinHandle` detaches the task rather than
        // aborting it, so without this hook every armed timer task would
        // keep sleeping — up to a turn deadline, which may be minutes —
        // after its actor is stopped. Settle them all on the way out.
        for waits in self.conversations.values_mut() {
            waits.settle_all();
        }
        Ok(())
    }
}

/// One inbound delivery, pumped from the transport inbox.
struct Deliver(kamiroh_ports::Delivery);

impl<T, R> Message<Deliver> for Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    type Reply = ();

    async fn handle(
        &mut self,
        Deliver(delivery): Deliver,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let self_address = self.address.clone();
        let origin = delivery.from.clone();
        match process(&self.allowlist, delivery) {
            Inbound::Denied => {
                self.runtime.emit(Event::DeliveryDenied {
                    from: origin,
                    to: self_address,
                });
            }
            Inbound::Request { reply_to, ack, .. } => {
                // Delivery acknowledged; no party is wired behind Plain
                // actors in v0. Send errors are dropped for now — the sender
                // times out rather than us crashing the host (spike scope).
                let _ = self.transport.send(&self_address, &reply_to, ack).await;
            }
            Inbound::AckReceived(ack) => {
                // The receipt we may have been waiting on: settle the wait.
                if let Some(waits) = self.conversations.get_mut(&origin) {
                    waits.settle_ack(ack.id);
                }
            }
            Inbound::Harness { harness, reply_to } => {
                let reply = if self.kind != ActorKind::Harness {
                    Some(Vocab::Harness(Harness::Failed {
                        reason: "not a harness actor".into(),
                    }))
                } else {
                    self.execute(harness, &reply_to)
                };
                if let Some(reply) = reply {
                    let _ = self.transport.send(&self_address, &reply_to, reply).await;
                }
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
                {
                    let waits = self.conversations.entry(reply_to.clone()).or_default();
                    match waits.state.on_incoming(&turn) {
                        Ok(_) => {
                            // Their turn is what we were waiting for — and it
                            // supersedes a still-missing ack.
                            waits.settle_all();
                        }
                        Err(error) => {
                            self.runtime.emit(Event::IllegalTurnDropped {
                                from: reply_to,
                                to: self_address,
                                error,
                            });
                            return;
                        }
                    }
                }
                // Ack on handover — the fast receipt, before the party thinks.
                if let Some(ack) = ack {
                    let _ = self.transport.send(&self_address, &reply_to, ack).await;
                }
                // The party's state change completes before its reply is sent
                // (decision 17); kameo's mailbox serializes turns per actor.
                let reply = match &mut self.party {
                    Some(party) => party.on_turn_boxed(&reply_to, turn).await,
                    None => None,
                };
                if let Some(reply_turn) = reply {
                    let posed = reply_turn.request().map(|r| r.id);
                    let progress = {
                        let waits = self.conversations.entry(reply_to.clone()).or_default();
                        waits.state.on_outgoing(&reply_turn).ok()
                    };
                    let Some(progress) = progress else {
                        // The party's own reply was illegal: dropped, and the
                        // exchange fails loudly rather than lingering with no
                        // deadline armed (decision 26).
                        self.fail_exchange(&reply_to, FailureCause::IllegalReply)
                            .await;
                        return;
                    };
                    {
                        let sent = self
                            .transport
                            .send(&self_address, &reply_to, Vocab::Turn(reply_turn))
                            .await;
                        if sent.is_err() {
                            // The transport refused the reply: positive local
                            // evidence it never left, so the exchange fails
                            // at once (decision 26).
                            self.fail_exchange(&reply_to, FailureCause::SendFailed)
                                .await;
                            return;
                        }
                        {
                            // Arm the waits this reply creates (decision 22):
                            // the ack of its request half, and (unless it
                            // closed the exchange) the peer's next turn.
                            let me = ctx.actor_ref().clone();
                            let waits = self.conversations.entry(reply_to.clone()).or_default();
                            waits.epoch = waits.epoch.wrapping_add(1);
                            let epoch = waits.epoch;
                            if let Some(id) = posed {
                                let peer = reply_to.clone();
                                let me = me.clone();
                                let after = self.deadlines.ack;
                                waits.ack = Some((
                                    id,
                                    tokio::spawn(async move {
                                        tokio::time::sleep(after).await;
                                        let _ = me
                                            .tell(DeadlineElapsed {
                                                peer,
                                                kind: DeadlineKind::Ack,
                                                ack_id: Some(id),
                                                epoch,
                                            })
                                            .await;
                                    }),
                                ));
                            }
                            waits.turn = match progress {
                                TurnProgress::Continuing => {
                                    let peer = reply_to.clone();
                                    let after = self.deadlines.turn;
                                    Some(tokio::spawn(async move {
                                        tokio::time::sleep(after).await;
                                        let _ = me
                                            .tell(DeadlineElapsed {
                                                peer,
                                                kind: DeadlineKind::Turn,
                                                ack_id: None,
                                                epoch,
                                            })
                                            .await;
                                    }))
                                }
                                TurnProgress::Concluded => None,
                            };
                        }
                    }
                }
            }
        }
    }
}

impl<T, R> Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    /// Fail one conversation's exchange (decision 23) and be loud about it
    /// (decision 25): the machine moves to `Failed`, armed timer tasks are
    /// settled, the party is told, and the observer sees the event. One home
    /// for the path shared by elapsed deadlines, refused sends, and illegal
    /// replies.
    async fn fail_exchange(&mut self, peer: &Address, cause: FailureCause) {
        if let Some(waits) = self.conversations.get_mut(peer) {
            waits.state.fail();
            waits.settle_all();
            // Insurance at the seam (the pre-flight hazards note): every
            // fail must leave queued stale DeadlineElapsed reports
            // recognizable. settle_all() already clears the waits they
            // re-check, and every re-arm bumps the epoch — but that
            // protection is emergent; bumping here makes it local, so a
            // future path through this seam cannot silently lose it.
            waits.epoch = waits.epoch.wrapping_add(1);
        }
        if let Some(party) = &mut self.party {
            party.on_exchange_failed_boxed(peer, cause).await;
        }
        self.runtime.emit(Event::ExchangeFailed {
            local: self.address.clone(),
            peer: peer.clone(),
            cause,
        });
    }

    /// The mutation itself, callable from any of this host's own handlers
    /// without a mailbox round-trip — deliberately separate from the
    /// [`Admit`]/[`Revoke`] messages so a future *internal* caller (the
    /// wire form's harness handler, workshop-3) cannot self-deadlock by
    /// `ask`ing the mailbox it is currently occupying.
    fn admit_here(&mut self, endpoint: EndpointId) {
        self.allowlist.admit(endpoint);
    }

    /// See [`Host::admit_here`] for why this is not inside the message
    /// handler. Fails live exchanges only — the same pre-filter as
    /// [`PeerDied`], and it is what keeps R5 true at the observer: an
    /// already-failed or idle conversation is not re-failed, so a second
    /// revocation emits nothing.
    async fn revoke_here(&mut self, endpoint: &EndpointId) -> usize {
        self.allowlist.revoke(endpoint);
        let affected: Vec<Address> = self
            .conversations
            .iter()
            .filter(|(peer, waits)| {
                peer.endpoint == *endpoint
                    && matches!(
                        waits.state,
                        TurnState::AwaitingTheirTurn { .. } | TurnState::OweThem { .. }
                    )
            })
            .map(|(peer, _)| peer.clone())
            .collect();
        for peer in &affected {
            self.fail_exchange(peer, FailureCause::Revoked).await;
        }
        affected.len()
    }
}

/// Admit an endpoint on this host's allowlist (decision 28).
struct Admit {
    endpoint: EndpointId,
}

impl<T, R> Message<Admit> for Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    type Reply = ();

    async fn handle(
        &mut self,
        Admit { endpoint }: Admit,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.admit_here(endpoint);
    }
}

/// Revoke an endpoint from this host's allowlist (decision 28). Replies
/// with the number of live exchanges failed.
struct Revoke {
    endpoint: EndpointId,
}

impl<T, R> Message<Revoke> for Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    type Reply = usize;

    async fn handle(
        &mut self,
        Revoke { endpoint }: Revoke,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.revoke_here(&endpoint).await
    }
}

/// A timer task's report that a deadline it was armed for has elapsed.
struct DeadlineElapsed {
    peer: Address,
    kind: DeadlineKind,
    /// For an ack deadline: the request id whose receipt was awaited, so a
    /// stale report (the wait was settled and the task aborted, but its
    /// message already queued) is recognized and ignored.
    ack_id: Option<RequestId>,
    /// The arming round this task belonged to (see [`PeerWaits::epoch`]).
    epoch: u64,
}

impl<T, R> Message<DeadlineElapsed> for Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    type Reply = ();

    async fn handle(
        &mut self,
        DeadlineElapsed {
            peer,
            kind,
            ack_id,
            epoch,
        }: DeadlineElapsed,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let Some(waits) = self.conversations.get_mut(&peer) else {
            return;
        };
        // Ignore stale reports: the wait this task was armed for must still
        // be pending.
        let still_pending = epoch == waits.epoch
            && match (kind, ack_id) {
                (DeadlineKind::Ack, Some(id)) => matches!(waits.ack, Some((due, _)) if due == id),
                (DeadlineKind::Turn, _) => waits.turn.is_some(),
                _ => false,
            };
        if !still_pending {
            return;
        }
        // The verdict (decision 23): the exchange is dead. Loudly (25).
        self.fail_exchange(&peer, FailureCause::DeadlineElapsed(kind))
            .await;
    }
}

/// The transport reported an endpoint dead (decision 27), fanned out by
/// [`KameoRuntime::watch_deaths`].
struct PeerDied {
    endpoint: EndpointId,
}

impl<T, R> Message<PeerDied> for Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    type Reply = ();

    async fn handle(
        &mut self,
        PeerDied { endpoint }: PeerDied,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        // Positive evidence fails live exchanges only (R6/R7): the
        // conversations themselves survive — a conversation spans
        // connections, and the peer may come back.
        let affected: Vec<Address> = self
            .conversations
            .iter()
            .filter(|(peer, waits)| {
                peer.endpoint == endpoint
                    && matches!(
                        waits.state,
                        TurnState::AwaitingTheirTurn { .. } | TurnState::OweThem { .. }
                    )
            })
            .map(|(peer, _)| peer.clone())
            .collect();
        for peer in affected {
            self.fail_exchange(&peer, FailureCause::Disconnected).await;
        }
    }
}

/// The [`Timer`] port on tokio time, for pumps and Phones hosted alongside
/// this runtime. Under `tokio::test(start_paused = true)` it is fully
/// deterministic — tokio auto-advances past sleeps.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioTimer;

impl Timer for TokioTimer {
    type Instant = tokio::time::Instant;

    fn now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }

    fn sleep_until(
        &self,
        deadline: tokio::time::Instant,
    ) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep_until(deadline)
    }
}

impl<T, R> Host<T, R>
where
    T: Transport + Clone + Send + Sync + 'static,
    R: Registry + Send + 'static,
    R::Inbox: Send + 'static,
{
    /// Execute a harness command via the runtime. Mirrors
    /// `LocalRuntime::execute`; reply kinds arriving here are ignored.
    fn execute(&self, command: Harness, controller: &Address) -> Option<Vocab> {
        let reply = match command {
            Harness::Ping => Harness::Pong,
            Harness::Spawn { name } => {
                // The spawned actor admits the controlling endpoint only,
                // and gets an EchoParty behind it — the first real Party.
                let mut allowlist = Allowlist::empty();
                allowlist.admit(controller.endpoint.clone());
                match self.runtime.install_party(
                    name.clone(),
                    allowlist,
                    self.runtime.inner.spawn_deadlines,
                    Box::new(EchoParty::new()),
                ) {
                    Ok(()) => Harness::Spawned { name },
                    Err(e) => Harness::Failed {
                        reason: e.to_string(),
                    },
                }
            }
            Harness::Stop { name } => match self.runtime.stop(&name) {
                Ok(()) => Harness::Stopped { name },
                Err(_) => Harness::Failed {
                    reason: "no such actor".into(),
                },
            },
            Harness::Spawned { .. }
            | Harness::Stopped { .. }
            | Harness::Pong
            | Harness::Failed { .. } => return None,
        };
        Some(Vocab::Harness(reply))
    }
}
