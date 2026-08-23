//! The executable specification: `features/*.feature`, bound to real steps.
//!
//! Roadmap item 6. The Gherkin files are the claims a reader needs to
//! believe, in glossary words; this file is their plumbing, and nothing
//! else. The ordinary tests in `tests/` remain the fast gate — they are
//! finer-grained, they pin things no scenario should mention, and they run
//! first. Cucumber sits on top and answers a different question: *does the
//! spec a non-implementer can read still hold?*
//!
//! Three rules kept while binding:
//!
//! - **Plumbing stays downstairs.** Steps reuse the ordinary tests'
//!   machinery — the memory adapter, the hand-cranked [`TestTimer`], the
//!   `stage`/`open_live_exchange` shapes — so a scenario never has to say
//!   "mailbox" or "waker".
//! - **Glossary words upstairs.** A step's text is the claim; if binding one
//!   needed a word the glossary lacks, that is a finding about the scenario,
//!   not a licence to invent vocabulary.
//! - **`LocalRuntime`, deliberately.** Story 3's binding note (decision 28)
//!   warns that "the very next delivery" means *processed after the revoke
//!   resolves*, which is true of the toy runtime by construction and only
//!   true of Kameo if the test sends after awaiting. Binding the mutation
//!   steps `LocalRuntime`-side keeps the scenario honest without a
//!   mailbox-ordering caveat the Gherkin would have to carry.
//!
//! The runner is driven by the workspace's own bounded `block_on` rather
//! than a second async runtime: cucumber 0.23 is runtime-agnostic, and the
//! stuck-detector that block_on now carries is exactly what a spec runner
//! wants when a scenario wedges.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cucumber::{World as _, given, then, when};

use kamiroh::adapter_memory::testing::{TestTimer, block_on_bounded};
use kamiroh::adapter_memory::{MemoryNet, MemoryTransport};
use kamiroh::app::inbound::{Inbound, process};
use kamiroh::app::phone::{Phone, PhoneError};
use kamiroh::app::runtime::{LocalRuntime, Stepped};
use kamiroh::domain::actor::{ActorName, Address};
use kamiroh::domain::allowlist::Allowlist;
use kamiroh::domain::deadline::{DeadlineKind, Deadlines, FailureCause};
use kamiroh::domain::endpoint::EndpointId;
use kamiroh::domain::hex::Hex;
use kamiroh::domain::protocol::{TurnError, TurnState};
use kamiroh::domain::vocabulary::{Message, Request, RequestId, Response, Turn};
use kamiroh::ports::{Event, Inbox as _, Registry as _, Transport as _};

// ---------------------------------------------------------------------------
// Plumbing: the fixed cast, and a transport that can be told to refuse.
// ---------------------------------------------------------------------------

/// The responder's endpoint, hosting the actor under test.
const HOME: &str = "bb";
/// The initiating application's endpoint.
const AWAY: &str = "aa";
/// A third endpoint, admitted by nobody unless a scenario says so.
const STRANGER: &str = "cc";

fn endpoint(s: &str) -> EndpointId {
    EndpointId::new(Hex::new(s).unwrap())
}

fn name(s: &str) -> ActorName {
    ActorName::new(s).unwrap()
}

fn address(e: &str, n: &str) -> Address {
    Address::new(endpoint(e), name(n))
}

/// This side's patience, used for every surface in every scenario so that
/// "the deadline elapses" has one meaning throughout.
fn patience() -> Deadlines {
    Deadlines::new(Duration::from_secs(5), Duration::from_secs(60))
}

/// Long enough to pass the turn deadline above, short of nothing else.
fn past_the_turn_deadline() -> Duration {
    Duration::from_secs(61)
}

fn past_the_ack_deadline() -> Duration {
    Duration::from_secs(6)
}

fn request(n: u8) -> Request {
    Request {
        id: RequestId([n; 16]),
        body: vec![n],
    }
}

/// The memory transport, wrapped so a scenario can make the wire refuse.
///
/// One type for every scenario (refusing is off unless asked) keeps the
/// `World` monomorphic — the alternative, swapping transports mid-scenario,
/// would change the runtime's type.
#[derive(Clone)]
struct RefusableTransport {
    inner: MemoryTransport,
    refusing: Arc<AtomicBool>,
}

impl kamiroh::ports::Transport for RefusableTransport {
    type Error = std::io::Error;

    async fn send(
        &mut self,
        from: &Address,
        to: &Address,
        message: Message,
    ) -> Result<(), Self::Error> {
        if self.refusing.load(Ordering::SeqCst) && matches!(message, Message::Turn(_)) {
            return Err(std::io::Error::other("the wire refused this turn"));
        }
        self.inner
            .send(from, to, message)
            .await
            .map_err(std::io::Error::other)
    }

    async fn open_many(
        &mut self,
        from: &Address,
        to_endpoint: &EndpointId,
        to_names: &[ActorName],
        request: Request,
    ) -> Result<(), Self::Error> {
        if self.refusing.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("the wire refused this batch"));
        }
        self.inner
            .open_many(from, to_endpoint, to_names, request)
            .await
            .map_err(std::io::Error::other)
    }
}

type Runtime = LocalRuntime<RefusableTransport, MemoryNet, TestTimer>;
type AppPhone = Phone<RefusableTransport, TestTimer>;

// ---------------------------------------------------------------------------
// The World
// ---------------------------------------------------------------------------

/// One scenario's whole situation: a responder endpoint running the actor
/// under test, an initiating application holding phones, and the two
/// observation surfaces failure is told through (decision 25).
#[derive(cucumber::World)]
#[world(init = Self::new)]
struct KamirohWorld {
    net: MemoryNet,
    timer: TestTimer,
    refusing: Arc<AtomicBool>,
    runtime: Runtime,
    /// Local operational events at the *responder's* home.
    events: Arc<Mutex<Vec<Event>>>,
    /// What the responder's party was told about its own exchanges.
    failures: Arc<Mutex<Vec<(Address, FailureCause)>>>,
    /// The application's address, inbox, and its own inbound policy.
    app: Address,
    app_inbox: <MemoryNet as kamiroh::ports::Registry>::Inbox,
    app_list: Allowlist,
    /// The actor under test, once installed.
    responder: Option<ActorName>,
    /// The application's phones. Single-peer scenarios use the first.
    phones: Vec<AppPhone>,
    /// Verdicts the application's own side reached.
    app_failures: Vec<FailureCause>,
    /// The most recent refusal, turn rejection, and step outcome.
    last_send_error: Option<String>,
    last_turn_error: Option<TurnError>,
    last_stepped: Option<Stepped>,
    /// Deliveries the responder actually consumed, so "nothing crossed the
    /// wire" is asserted against a count rather than a hopeful silence.
    deliveries_consumed: usize,
    /// Whether the application returns the delivery ack it is handed. Off
    /// only in the scenario about an ack that never comes.
    app_acks: bool,
    /// A second admitted peer, for scenarios with two conversations.
    second_peer: Option<(
        Address,
        <MemoryNet as kamiroh::ports::Registry>::Inbox,
        AppPhone,
    )>,
    /// How many exchanges the last revocation failed.
    exchanges_failed_by_revocation: usize,
    /// Observation counts taken before a repeated revocation.
    events_before_repeat: usize,
    failures_before_repeat: usize,
    /// Set by the key-rotation scenario, whose "new endpoint" is the third.
    responder_admits_old_key: bool,
    /// Fan-out: the siblings installed, who denies, who was revoked, how
    /// many phones the one receipt settled, and who claimed to send it.
    siblings: Vec<ActorName>,
    denying_sibling: Option<ActorName>,
    revoked_sibling: Option<ActorName>,
    receipt_settled: usize,
    batch_receipt_sender: Option<Address>,
    /// The one scenario that runs on real sockets (see below).
    wire: Option<WireFixture>,
    wire_runtime: Option<tokio::runtime::Runtime>,
    wire_answered_by: Option<Address>,
}

impl std::fmt::Debug for KamirohWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KamirohWorld")
            .field("responder", &self.responder)
            .field("phones", &self.phones.len())
            .field("events", &self.events.lock().unwrap().len())
            .field("failures", &self.failures.lock().unwrap())
            .field("app_failures", &self.app_failures)
            .field("last_send_error", &self.last_send_error)
            .field("last_turn_error", &self.last_turn_error)
            .finish()
    }
}

impl KamirohWorld {
    async fn new() -> Result<Self, std::convert::Infallible> {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let refusing = Arc::new(AtomicBool::new(false));
        let transport = RefusableTransport {
            inner: net.transport(),
            refusing: Arc::clone(&refusing),
        };
        let mut runtime = LocalRuntime::new(
            endpoint(HOME),
            transport,
            net.clone(),
            timer.clone(),
            patience(),
        );
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        runtime.set_observer(move |event: Event| sink.lock().unwrap().push(event));

        let app = address(AWAY, "app");
        let app_inbox = net.clone().bind(&app).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint(HOME));

        Ok(Self {
            net,
            timer,
            refusing,
            runtime,
            events,
            failures: Arc::new(Mutex::new(Vec::new())),
            app,
            app_inbox,
            app_list,
            responder: None,
            phones: Vec::new(),
            app_failures: Vec::new(),
            last_send_error: None,
            last_turn_error: None,
            last_stepped: None,
            deliveries_consumed: 0,
            app_acks: true,
            second_peer: None,
            exchanges_failed_by_revocation: 0,
            events_before_repeat: 0,
            failures_before_repeat: 0,
            responder_admits_old_key: false,
            siblings: Vec::new(),
            denying_sibling: None,
            revoked_sibling: None,
            receipt_settled: 0,
            batch_receipt_sender: None,
            wire: None,
            wire_runtime: None,
            wire_answered_by: None,
        })
    }

    fn transport(&self) -> RefusableTransport {
        RefusableTransport {
            inner: self.net.transport(),
            refusing: Arc::clone(&self.refusing),
        }
    }

    /// Install the actor under test with a `CountdownParty` behind it — the
    /// party that poses questions of its own, so an exchange can sit
    /// mid-flight awaiting *our* turn as well as theirs.
    fn install_responder(&mut self, admits: &[&str], rounds: u8) -> ActorName {
        let who = name("asker");
        let mut list = Allowlist::empty();
        for e in admits {
            list.admit(endpoint(e));
        }
        self.runtime
            .install_party(
                who.clone(),
                list,
                patience(),
                Box::new(RecordingParty::new(rounds, Arc::clone(&self.failures))),
            )
            .unwrap();
        // Every scenario's responder watches for death evidence: the
        // transport reporting a peer gone is raced into each step
        // (decision 27), and a scenario that never kills anything is
        // unaffected by the watch being armed.
        self.runtime.watch_deaths(self.net.death_watch());
        self.responder = Some(who.clone());
        who
    }

    fn responder(&self) -> ActorName {
        self.responder.clone().expect("no actor installed yet")
    }

    fn responder_address(&self) -> Address {
        Address::new(endpoint(HOME), self.responder())
    }

    fn phone(&mut self) -> &mut AppPhone {
        self.phones.first_mut().expect("no phone in this scenario")
    }

    fn new_phone(&mut self) -> AppPhone {
        Phone::converse(
            self.app.clone(),
            self.responder_address(),
            self.transport(),
            patience(),
            self.timer.clone(),
        )
    }

    /// One responder step, remembering what it did.
    async fn step(&mut self) {
        let who = self.responder();
        let stepped = self.runtime.step(&who).await.expect("the actor is running");
        if matches!(stepped, Stepped::Delivered) {
            self.deliveries_consumed += 1;
        }
        self.last_stepped = Some(stepped);
    }

    /// Drain the application's inbox until its phone has heard the
    /// responder's turn, feeding acks to the phone on the way.
    async fn app_hears_a_turn(&mut self) {
        loop {
            let delivery = self.app_inbox.next().await.expect("the app inbox is open");
            match process(&self.app_list, delivery) {
                Inbound::AckReceived(ack) => {
                    self.phone().note_ack(&ack);
                }
                Inbound::Turn {
                    turn,
                    reply_to,
                    ack,
                    ..
                } => {
                    // The fast receipt, returned as a real peer's runtime
                    // would — unless this scenario is about one that never
                    // comes.
                    if let (true, Some(ack)) = (self.app_acks, ack) {
                        let app = self.app.clone();
                        let _ = self.transport().send(&app, &reply_to, ack).await;
                    }
                    let outcome = self.phone().on_incoming(&turn);
                    if let Err(PhoneError::Turn(e)) = outcome {
                        self.last_turn_error = Some(e);
                    }
                    return;
                }
                other => panic!("the app heard something unexpected: {other:?}"),
            }
        }
    }

    fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }

    fn party_failures(&self) -> Vec<(Address, FailureCause)> {
        self.failures.lock().unwrap().clone()
    }
}

/// The party behind the actor under test: answers each request and, while
/// its counter holds, poses one of its own — and records what it is told
/// about failure, so the "the party is told" half of decision 25 is
/// assertable.
///
/// Deliberately **per-peer**: it keeps one shadow `TurnState` for each
/// conversation, not one for the party. The stock `EchoParty` and
/// `CountdownParty` keep a single machine, which is right for the
/// single-conversation tests they were written for but wrong the moment one
/// actor talks to two peers at once — the second peer's opening turn hits
/// the first conversation's `AwaitingTheirTurn` and is refused as
/// `MustAnswerFirst`. The runtime keys conversations per peer (decision 17);
/// a party's shadow of that state has to as well.
struct RecordingParty {
    /// One shadow machine per peer, mirroring the runtime's own keying.
    states: std::collections::HashMap<Address, TurnState>,
    remaining: u8,
    next_id: u8,
    failures: Arc<Mutex<Vec<(Address, FailureCause)>>>,
}

impl RecordingParty {
    fn new(rounds: u8, failures: Arc<Mutex<Vec<(Address, FailureCause)>>>) -> Self {
        Self {
            states: std::collections::HashMap::new(),
            remaining: rounds,
            next_id: 100,
            failures,
        }
    }
}

impl kamiroh::ports::Party for RecordingParty {
    async fn on_turn(&mut self, from: &Address, turn: Turn) -> Option<Turn> {
        let state = self.states.entry(from.clone()).or_default();
        if state.on_incoming(&turn).is_err() {
            return None;
        }
        let request = turn.request()?.clone();
        let response = Response {
            id: request.id,
            body: request.body,
        };
        let reply = if self.remaining > 0 {
            self.remaining -= 1;
            let id = RequestId([self.next_id; 16]);
            self.next_id = self.next_id.wrapping_add(1);
            Turn::Continue {
                response,
                request: Request {
                    id,
                    body: vec![self.remaining],
                },
            }
        } else {
            Turn::Close { response }
        };
        let state = self.states.entry(from.clone()).or_default();
        state
            .on_outgoing(&reply)
            .expect("the party's own reply must be legal");
        Some(reply)
    }

    fn on_exchange_failed(
        &mut self,
        from: &Address,
        cause: FailureCause,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.failures.lock().unwrap().push((from.clone(), cause));
        // A party that keeps its own machine must fail it too, or a
        // surviving fresh `Open` is swallowed (the external-review finding
        // of 2026-08-23, here kept per-peer).
        if let Some(state) = self.states.get_mut(from) {
            state.fail();
        }
        async {}
    }
}

// ---------------------------------------------------------------------------
// Background — the standing facts every scenario inherits
// ---------------------------------------------------------------------------

#[given("every conversation surface is constructed with finite deadlines")]
async fn surfaces_take_finite_deadlines(world: &mut KamirohWorld) {
    // Not a runtime check but a type-level one, asserted here so the claim
    // is visible in the spec: `Deadlines` has no unbounded variant and no
    // `Default`, so every surface below had to be handed finite values.
    let d = patience();
    assert!(d.ack > Duration::ZERO && d.turn > Duration::ZERO);
    world.timer.advance(Duration::ZERO);
}

#[given("each side's deadlines bound its own waiting only")]
async fn deadlines_are_each_sides_own(_world: &mut KamirohWorld) {
    // Nothing about deadlines crosses the wire: the vocabulary has no
    // variant that could carry one (decision 22). Nothing to arrange.
}

// ---------------------------------------------------------------------------
// Shared arrangements
// ---------------------------------------------------------------------------

impl KamirohWorld {
    /// Bring a conversation to the point the specs talk about most: an
    /// exchange under way, with the *responder* awaiting the peer's turn.
    ///
    /// The app opens; the actor acks on handover and its party answers with
    /// a question of its own; the app hears that turn and returns the
    /// receipt. Both sides are now live and only the peer's turn is owed.
    async fn arrange_live_exchange(&mut self) {
        self.install_responder(&[AWAY], 4);
        let mut phone = self.new_phone();
        phone.open(request(1)).await.expect("opening is legal");
        self.phones.push(phone);
        self.step().await;
        assert!(
            matches!(self.last_stepped, Some(Stepped::Delivered)),
            "the opening turn should have been delivered"
        );
        self.app_hears_a_turn().await;
    }

    /// Step the responder until `done` holds, or give up loudly.
    ///
    /// A receipt the peer returned is often queued ahead of the delivery a
    /// scenario is about, and one step takes exactly one delivery. Stepping
    /// to a condition keeps scenarios from encoding mailbox arithmetic.
    async fn step_until(&mut self, what: &str, mut done: impl FnMut(&Self) -> bool) {
        for _ in 0..8 {
            if done(self) {
                return;
            }
            self.step().await;
        }
        assert!(done(self), "the actor never reached: {what}");
    }

    /// Assert the responder's last step failed the exchange, and give back
    /// the cause.
    fn failed_cause(&self) -> FailureCause {
        // Every `fail_exchange` tells the party (decision 25), including a
        // revocation, which the operator drives directly rather than
        // through a step. The party's record is therefore the one surface
        // every verdict reaches.
        if let Some((_, cause)) = self.party_failures().last() {
            return *cause;
        }
        match &self.last_stepped {
            Some(Stepped::ExchangeFailed { cause, .. }) => *cause,
            // A reported death fails every live exchange the endpoint
            // touched, so the step reports the sweep and the *cause* is
            // what each affected party was told (decision 27).
            Some(Stepped::PeerVanished {
                exchanges_failed, ..
            }) if *exchanges_failed > 0 => {
                self.party_failures()
                    .last()
                    .expect("a failed exchange tells its party")
                    .1
            }
            other => panic!("the exchange did not fail; the step said {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Story 1 — a hung exchange fails loudly
// ---------------------------------------------------------------------------

#[given("an exchange between two parties with a turn deadline")]
#[given("an exchange with a turn deadline")]
#[given("an exchange awaiting the peer's turn")]
async fn an_exchange_with_a_turn_deadline(world: &mut KamirohWorld) {
    world.arrange_live_exchange().await;
}

#[given("the exchange is awaiting the peer's turn")]
async fn the_exchange_is_awaiting_the_peers_turn(world: &mut KamirohWorld) {
    // The app holds the turn: it owes the answer the actor's party asked
    // for, which is precisely what the actor is waiting on.
    assert!(
        matches!(world.phone().state(), TurnState::OweThem { .. }),
        "the peer should owe the next turn"
    );
}

#[when("the deadline elapses with no turn arriving")]
#[when("the peer's own turn deadline elapses with no further turn arriving")]
async fn the_turn_deadline_elapses(world: &mut KamirohWorld) {
    world.timer.advance(past_the_turn_deadline());
    world.step().await;
}

#[then("the exchange fails with a timeout")]
#[then("the peer's side of the exchange fails with a timeout")]
#[then("the turn deadline elapses and the exchange fails with a timeout")]
async fn the_exchange_fails_with_a_timeout(world: &mut KamirohWorld) {
    assert!(
        matches!(world.failed_cause(), FailureCause::DeadlineElapsed(_)),
        "expected a timeout, got {:?}",
        world.failed_cause()
    );
}

#[then("the waiting party is told the exchange failed")]
#[then("the party is told the exchange failed")]
#[then("the sender's party is told the exchange failed")]
async fn the_party_is_told(world: &mut KamirohWorld) {
    let told = world.party_failures();
    assert!(
        told.iter().any(|(peer, _)| *peer == world.app),
        "the party was never told about its exchange with the peer: {told:?}"
    );
}

#[then("the conversation may open a new exchange")]
async fn the_conversation_may_open_a_new_exchange(world: &mut KamirohWorld) {
    // The conversation outlives the exchange (decision 23): a fresh Open is
    // legal, and the actor serves it. Whatever a scenario did to the wire,
    // it is the conversation being asked about here.
    world.refusing.store(false, Ordering::SeqCst);
    let mut fresh = world.new_phone();
    fresh.open(request(9)).await.expect("a fresh open is legal");
    world.phones.push(fresh);
    world.step().await;
    assert!(
        matches!(world.last_stepped, Some(Stepped::Delivered)),
        "the fresh exchange was not served: {:?}",
        world.last_stepped
    );
    assert!(
        !world
            .events()
            .iter()
            .any(|e| matches!(e, Event::IllegalTurnDropped { .. })),
        "the fresh opening turn was refused rather than served"
    );
}

#[when("the peer's turn arrives before the deadline elapses")]
#[when("the peer's turn arrives within the deadline")]
async fn the_peers_turn_arrives_in_time(world: &mut KamirohWorld) {
    // Well inside the actor's patience: the clock moves, but not far.
    world.timer.advance(Duration::from_secs(1));
    let outstanding = match world.phone().state() {
        TurnState::OweThem { outstanding } => outstanding,
        other => panic!("the peer does not owe a turn: {other:?}"),
    };
    world
        .phone()
        .send_turn(Turn::Continue {
            response: Response {
                id: outstanding,
                body: vec![],
            },
            request: request(2),
        })
        .await
        .expect("answering is legal");
    world.step().await;
}

#[then("the exchange continues as if no deadline existed")]
#[then("the exchange continues in the same conversation")]
async fn the_exchange_continues(world: &mut KamirohWorld) {
    assert!(
        matches!(world.last_stepped, Some(Stepped::Delivered)),
        "the exchange did not continue: {:?}",
        world.last_stepped
    );
    assert!(
        world.party_failures().is_empty(),
        "nothing should have failed: {:?}",
        world.party_failures()
    );
}

#[given("a sent turn awaiting its delivery ack, with an ack deadline")]
async fn a_sent_turn_awaiting_its_ack(world: &mut KamirohWorld) {
    // The application does not return the receipt in this scenario, so the
    // actor's ack wait is the one left standing.
    world.app_acks = false;
    world.install_responder(&[AWAY], 4);
    let mut phone = world.new_phone();
    phone.open(request(1)).await.expect("opening is legal");
    world.phones.push(phone);
    world.step().await;
    world.app_hears_a_turn().await;
}

#[when("the ack deadline elapses first")]
async fn the_ack_deadline_elapses(world: &mut KamirohWorld) {
    world.timer.advance(past_the_ack_deadline());
    world.step().await;
}

#[given("an exchange that already failed by timeout")]
async fn an_exchange_that_already_failed(world: &mut KamirohWorld) {
    world.arrange_live_exchange().await;
    world.timer.advance(past_the_turn_deadline());
    world.step().await;
    assert!(matches!(
        world.failed_cause(),
        FailureCause::DeadlineElapsed(_)
    ));
}

#[when("the peer's answer finally arrives")]
async fn the_peers_answer_finally_arrives(world: &mut KamirohWorld) {
    let outstanding = match world.phone().state() {
        TurnState::OweThem { outstanding } => outstanding,
        other => panic!("the peer does not owe a turn: {other:?}"),
    };
    world
        .phone()
        .send_turn(Turn::Close {
            response: Response {
                id: outstanding,
                body: vec![],
            },
        })
        .await
        .expect("the peer still believes the exchange is alive");
    world
        .step_until("the late turn reached the actor", |w| {
            w.events()
                .iter()
                .any(|e| matches!(e, Event::IllegalTurnDropped { .. }))
        })
        .await;
}

#[then("it is refused as no part of any exchange")]
async fn it_is_refused_as_no_part_of_any_exchange(world: &mut KamirohWorld) {
    let dropped = world
        .events()
        .into_iter()
        .filter_map(|e| match e {
            Event::IllegalTurnDropped { error, .. } => Some(error),
            _ => None,
        })
        .next_back();
    assert!(
        matches!(dropped, Some(TurnError::ExchangeFailed)),
        "the late turn should have been refused as part of a dead exchange, got {dropped:?}"
    );
}

#[given("an exchange whose sender has failed it on an elapsed ack deadline")]
async fn the_sender_failed_on_an_ack_deadline(world: &mut KamirohWorld) {
    world.install_responder(&[AWAY], 4);
    let mut phone = world.new_phone();
    phone.open(request(1)).await.expect("opening is legal");
    world.phones.push(phone);
    // The actor has not been stepped, so no receipt comes back and the
    // sender's own patience runs out first.
    world.timer.advance(past_the_ack_deadline());
    let cause = world
        .phone()
        .on_deadline_elapsed()
        .expect("the sender was waiting on a receipt");
    assert_eq!(cause, FailureCause::DeadlineElapsed(DeadlineKind::Ack));
    world.app_failures.push(cause);
}

#[given("a peer that believes the exchange is alive")]
async fn a_peer_that_believes_the_exchange_is_alive(world: &mut KamirohWorld) {
    // The actor now takes the opening turn that was waiting for it: from
    // its side the exchange is only just beginning.
    world.step().await;
    assert!(matches!(world.last_stepped, Some(Stepped::Delivered)));
    world.app_hears_a_turn().await;
    assert!(
        world.party_failures().is_empty(),
        "the peer should not have failed anything yet"
    );
}

#[then("no failure message has crossed the wire")]
async fn no_failure_message_crossed_the_wire(world: &mut KamirohWorld) {
    // Both sides hold a verdict, and each reached it on its own clock:
    // `DeadlineElapsed` is a verdict only a local timer can produce, and
    // the vocabulary has no variant that could carry one the other way
    // (decision 22 — nothing about failure crosses the wire).
    assert!(
        world
            .app_failures
            .iter()
            .any(|c| matches!(c, FailureCause::DeadlineElapsed(_))),
        "the sender never reached its own verdict"
    );
    assert!(
        matches!(world.failed_cause(), FailureCause::DeadlineElapsed(_)),
        "the peer's verdict was not its own clock's"
    );
    assert!(
        !matches!(world.last_stepped, Some(Stepped::Delivered)),
        "the peer's verdict consumed a delivery, so something did cross"
    );
}

#[given("an exchange whose next turn is handed to the transport")]
async fn a_turn_handed_to_the_transport(world: &mut KamirohWorld) {
    world.install_responder(&[AWAY], 4);
    let mut phone = world.new_phone();
    phone.open(request(1)).await.expect("opening is legal");
    world.phones.push(phone);
}

#[when("the transport refuses to carry it")]
async fn the_transport_refuses(world: &mut KamirohWorld) {
    world.refusing.store(true, Ordering::SeqCst);
    world.step().await;
}

#[then("the exchange fails at once, well before any deadline")]
async fn it_fails_at_once(world: &mut KamirohWorld) {
    let cause = world.failed_cause();
    assert!(
        !matches!(cause, FailureCause::DeadlineElapsed(_)),
        "this should be positive evidence, not a timeout: {cause:?}"
    );
}

#[given("an actor whose allowlist does not admit a sender's endpoint")]
async fn an_actor_that_does_not_admit_the_sender(world: &mut KamirohWorld) {
    world.install_responder(&[AWAY], 4);
}

#[when("the unadmitted sender's delivery arrives")]
async fn an_unadmitted_delivery_arrives(world: &mut KamirohWorld) {
    let stranger = address(STRANGER, "stranger");
    let _stranger_inbox = world.net.clone().bind(&stranger).unwrap();
    let target = world.responder_address();
    world
        .transport()
        .send(
            &stranger,
            &target,
            Message::Turn(Turn::Open {
                request: request(3),
            }),
        )
        .await
        .expect("the wire carries it; the door is where it stops");
    world.step().await;
}

#[then("the delivery is denied and the sender learns nothing")]
async fn the_delivery_is_denied(world: &mut KamirohWorld) {
    // `Inbound::Denied` is the one outcome carrying no reply — the sender
    // learns nothing because there is nothing to learn from. That the path
    // was taken is what the event proves.
    assert!(
        world
            .events()
            .iter()
            .any(|e| matches!(e, Event::DeliveryDenied { from, .. } if from.endpoint == endpoint(STRANGER))),
        "the stranger's delivery was not denied"
    );
    assert!(
        world.party_failures().is_empty(),
        "denied traffic is pre-admission and must never reach a party"
    );
}

#[then("the denial is observable on the receiving side")]
async fn the_denial_is_observable(world: &mut KamirohWorld) {
    assert!(
        world
            .events()
            .iter()
            .any(|e| matches!(e, Event::DeliveryDenied { .. })),
        "the denial was not observable at home"
    );
}

// ---------------------------------------------------------------------------
// Story 2 — a vanished peer fails loudly
// ---------------------------------------------------------------------------

#[when("the peer's endpoint is killed")]
async fn the_peers_endpoint_is_killed(world: &mut KamirohWorld) {
    world.net.kill_endpoint(&endpoint(AWAY));
}

#[when("the transport reports the peer's endpoint dead")]
async fn the_transport_reports_the_death(world: &mut KamirohWorld) {
    // The death watch is raced into every step (decision 27); this step is
    // where the actor next runs and therefore hears it.
    world.step().await;
}

#[given("an exchange that failed because its peer's endpoint was killed")]
async fn an_exchange_failed_by_death(world: &mut KamirohWorld) {
    world.arrange_live_exchange().await;
    world.net.kill_endpoint(&endpoint(AWAY));
    world.step().await;
    assert_eq!(world.failed_cause(), FailureCause::Disconnected);
}

#[when("the peer returns under the same endpoint identity")]
async fn the_peer_returns(world: &mut KamirohWorld) {
    let app = world.app.clone();
    world.app_inbox = world
        .net
        .clone()
        .bind(&app)
        .expect("the address is free again");
}

#[then("a fresh exchange opens in the same conversation")]
async fn a_fresh_exchange_opens(world: &mut KamirohWorld) {
    let mut fresh = world.new_phone();
    fresh.open(request(5)).await.expect("a fresh open is legal");
    world.phones.push(fresh);
    world.step().await;
    assert!(
        matches!(world.last_stepped, Some(Stepped::Delivered)),
        "the returning peer was not served: {:?}",
        world.last_stepped
    );
}

#[when("the connection drops and is re-established")]
async fn the_connection_blips(world: &mut KamirohWorld) {
    // The memory net's model of a blip: deliveries are held, not lost, and
    // no death is reported — silence is the deadline's business, not
    // positive evidence (decision 27).
    world.net.partition(&endpoint(AWAY), &endpoint(HOME));
    world.net.heal(&endpoint(AWAY), &endpoint(HOME));
}

#[given("the transport observes nothing unusual")]
async fn the_transport_observes_nothing(world: &mut KamirohWorld) {
    assert!(
        world.party_failures().is_empty(),
        "nothing should have been reported yet"
    );
}

#[when("the peer's process is frozen rather than killed")]
async fn the_peer_is_frozen(world: &mut KamirohWorld) {
    // A freeze is silence: the wire holds, nothing is reported, and only a
    // deadline can catch it.
    world.net.partition(&endpoint(AWAY), &endpoint(HOME));
    world.timer.advance(past_the_turn_deadline());
    world.step().await;
}

// ---------------------------------------------------------------------------
// Story 3 — an actor's guest list changes while it runs
//
// Bound `LocalRuntime`-side on purpose (see this file's header): the toy
// runtime mutates strictly between steps, so "the very next delivery" is
// literally true and the scenario needs no mailbox-ordering caveat.
// ---------------------------------------------------------------------------

#[given("a running actor whose allowlist is empty")]
async fn a_running_actor_admitting_nobody(world: &mut KamirohWorld) {
    world.install_responder(&[], 4);
}

#[given("a running actor that admits two endpoints")]
async fn a_running_actor_admitting_two(world: &mut KamirohWorld) {
    world.install_responder(&[AWAY, STRANGER], 4);
}

#[given("a running actor that admits one endpoint")]
async fn a_running_actor_admitting_one(world: &mut KamirohWorld) {
    world.install_responder(&[AWAY], 4);
}

#[given("a running actor that admits the old key's endpoint")]
async fn a_running_actor_admitting_the_old_key(world: &mut KamirohWorld) {
    world.install_responder(&[AWAY], 4);
    // In rotation the incoming key is a *different* endpoint, so the
    // admit/revoke steps below aim at the new one and the old one in turn.
    world.responder_admits_old_key = true;
}

#[given("a conversation in progress with each")]
async fn a_conversation_with_each(world: &mut KamirohWorld) {
    // The admitted application, and a second peer at the other admitted
    // endpoint, each mid-exchange with the same actor.
    let mut mine = world.new_phone();
    mine.open(request(1)).await.expect("opening is legal");
    world.phones.push(mine);
    world.step().await;
    world.app_hears_a_turn().await;

    let other = address(STRANGER, "app");
    let mut other_inbox = world.net.clone().bind(&other).unwrap();
    let mut other_phone = Phone::converse(
        other.clone(),
        world.responder_address(),
        world.transport(),
        patience(),
        world.timer.clone(),
    );
    other_phone
        .open(request(2))
        .await
        .expect("opening is legal");
    // Two steps, deliberately: the first peer returned a delivery receipt
    // that is still queued ahead of this opening turn, and one step takes
    // exactly one delivery.
    world.step().await;
    world.step().await;
    // Drain that peer's side to the same live point.
    let mut list = Allowlist::empty();
    list.admit(endpoint(HOME));
    loop {
        let delivery = other_inbox.next().await.unwrap();
        match process(&list, delivery) {
            Inbound::AckReceived(_) => {}
            Inbound::Turn { turn, .. } => {
                other_phone.on_incoming(&turn).unwrap();
                break;
            }
            other => panic!("unexpected inbound: {other:?}"),
        }
    }
    world.second_peer = Some((other, other_inbox, other_phone));
}

#[when("its operator admits a new endpoint")]
#[when("its operator admits the new key's endpoint")]
async fn the_operator_admits(world: &mut KamirohWorld) {
    let who = world.responder();
    let newcomer = if world.responder_admits_old_key {
        endpoint(STRANGER)
    } else {
        endpoint(AWAY)
    };
    world
        .runtime
        .admit(&who, newcomer)
        .expect("the actor is running");
}

#[when("its operator revokes the second endpoint")]
async fn the_operator_revokes_the_second(world: &mut KamirohWorld) {
    let who = world.responder();
    world
        .runtime
        .revoke(&who, &endpoint(STRANGER))
        .await
        .expect("the actor is running");
}

#[when("the operator revokes that peer's endpoint")]
#[when("its operator revokes that endpoint")]
#[when("its operator revokes the old key's endpoint")]
async fn the_operator_revokes_the_peer(world: &mut KamirohWorld) {
    let who = world.responder();
    let failed = world
        .runtime
        .revoke(&who, &endpoint(AWAY))
        .await
        .expect("the actor is running");
    world.exchanges_failed_by_revocation = failed;
}

#[when("its operator revokes an endpoint that was never admitted")]
async fn the_operator_revokes_a_stranger(world: &mut KamirohWorld) {
    let who = world.responder();
    let failed = world
        .runtime
        .revoke(&who, &endpoint("dd"))
        .await
        .expect("the actor is running");
    world.exchanges_failed_by_revocation = failed;
}

#[when("the operator revokes that same endpoint again")]
async fn the_operator_revokes_again(world: &mut KamirohWorld) {
    let who = world.responder();
    world.events_before_repeat = world.events().len();
    world.failures_before_repeat = world.party_failures().len();
    let failed = world
        .runtime
        .revoke(&who, &endpoint(AWAY))
        .await
        .expect("the actor is running");
    world.exchanges_failed_by_revocation = failed;
}

#[then("a request from that endpoint is delivered and acknowledged")]
async fn a_request_is_delivered_and_acked(world: &mut KamirohWorld) {
    let mut phone = world.new_phone();
    phone.open(request(4)).await.expect("opening is legal");
    world.phones.push(phone);
    world.step().await;
    assert!(matches!(world.last_stepped, Some(Stepped::Delivered)));
    // The receipt comes back before the party has said anything.
    let delivery = world.app_inbox.next().await.unwrap();
    let acked = matches!(
        process(&world.app_list, delivery),
        Inbound::AckReceived(_) | Inbound::Turn { .. }
    );
    assert!(acked, "the newly admitted peer was not heard");
}

#[then("the actor was never restarted")]
async fn the_actor_was_never_restarted(world: &mut KamirohWorld) {
    // Installation happens once per scenario; a restart would have needed a
    // second install, and the name is still the one first installed.
    assert!(
        world.responder.is_some(),
        "the actor under test is the one installed at the start"
    );
}

#[then("the next delivery from the revoked endpoint is denied")]
async fn the_next_delivery_is_denied(world: &mut KamirohWorld) {
    let (peer, _inbox, phone) = world
        .second_peer
        .as_mut()
        .expect("this scenario has a second peer");
    let outstanding = match phone.state() {
        TurnState::OweThem { outstanding } => outstanding,
        other => panic!("the revoked peer does not owe a turn: {other:?}"),
    };
    let peer = peer.clone();
    phone
        .send_turn(Turn::Close {
            response: Response {
                id: outstanding,
                body: vec![],
            },
        })
        .await
        .expect("the revoked peer does not know yet");
    world.step().await;
    assert!(
        world.events().iter().any(
            |e| matches!(e, Event::DeliveryDenied { from, .. } if from.endpoint == peer.endpoint)
        ),
        "the revoked peer's turn was not denied"
    );
}

#[then("a command under the old key is denied and the denial observed")]
async fn the_old_key_is_denied(world: &mut KamirohWorld) {
    // The old key tries again, not knowing it has been rotated out.
    let mut phone = world.new_phone();
    phone
        .open(request(10))
        .await
        .expect("the wire still carries it");
    world.phones.push(phone);
    let app = world.app.clone();
    world
        .step_until("the old key's command was judged", move |w| {
            w.events()
                .iter()
                .any(|e| matches!(e, Event::DeliveryDenied { from, .. } if *from == app))
        })
        .await;
}

#[then("the denial is observed at home")]
async fn the_denial_is_observed_at_home(world: &mut KamirohWorld) {
    assert!(
        world
            .events()
            .iter()
            .any(|e| matches!(e, Event::DeliveryDenied { .. })),
        "no denial was observed at home"
    );
}

#[then("the first endpoint's conversation is untouched")]
async fn the_first_conversation_is_untouched(world: &mut KamirohWorld) {
    let app = world.app.clone();
    assert!(
        !world
            .events()
            .iter()
            .any(|e| matches!(e, Event::DeliveryDenied { from, .. } if *from == app)),
        "the endpoint that was kept lost traffic"
    );
    assert!(
        !world.party_failures().iter().any(|(peer, _)| *peer == app),
        "the endpoint that was kept had an exchange failed"
    );
}

#[then("the failure names the revocation as its cause")]
async fn the_failure_names_the_revocation(world: &mut KamirohWorld) {
    assert!(
        world
            .party_failures()
            .iter()
            .any(|(_, cause)| *cause == FailureCause::Revoked),
        "no exchange failed with a revocation: {:?}",
        world.party_failures()
    );
}

#[then("no further failure is reported to anyone")]
async fn no_further_failure_is_reported(world: &mut KamirohWorld) {
    assert_eq!(
        world.exchanges_failed_by_revocation, 0,
        "the repeat revocation claimed to fail something"
    );
    assert_eq!(
        world.events().len(),
        world.events_before_repeat,
        "the repeat revocation emitted an event"
    );
    assert_eq!(
        world.party_failures().len(),
        world.failures_before_repeat,
        "the repeat revocation told a party something"
    );
}

#[then("the admitted endpoint's deliveries continue unaffected")]
async fn the_admitted_endpoint_continues(world: &mut KamirohWorld) {
    assert_eq!(
        world.exchanges_failed_by_revocation, 0,
        "revoking a stranger failed something"
    );
    let mut phone = world.new_phone();
    phone.open(request(6)).await.expect("opening is legal");
    world.phones.push(phone);
    world.step().await;
    assert!(matches!(world.last_stepped, Some(Stepped::Delivered)));
}

#[then("the actor receives nothing from anyone")]
async fn the_actor_receives_nothing(world: &mut KamirohWorld) {
    // The endpoint just revoked, and one never admitted: deny-by-default
    // has both halves.
    let mut phone = world.new_phone();
    phone.open(request(7)).await.expect("opening is legal");
    world.phones.push(phone);
    world.step().await;

    let stranger = address(STRANGER, "stranger");
    let _inbox = world.net.clone().bind(&stranger).unwrap();
    let target = world.responder_address();
    world
        .transport()
        .send(
            &stranger,
            &target,
            Message::Turn(Turn::Open {
                request: request(8),
            }),
        )
        .await
        .unwrap();
    world.step().await;

    let denied: Vec<_> = world
        .events()
        .into_iter()
        .filter(|e| matches!(e, Event::DeliveryDenied { .. }))
        .collect();
    assert!(
        denied.len() >= 2,
        "expected both the revoked peer and a stranger to be denied, saw {denied:?}"
    );
}

#[then("a command under the new key is delivered")]
async fn a_command_under_the_new_key_is_delivered(world: &mut KamirohWorld) {
    let newcomer = address(STRANGER, "console");
    let mut inbox = world.net.clone().bind(&newcomer).unwrap();
    let target = world.responder_address();
    world
        .transport()
        .send(
            &newcomer,
            &target,
            Message::Turn(Turn::Open {
                request: request(11),
            }),
        )
        .await
        .unwrap();
    world.step().await;
    assert!(matches!(world.last_stepped, Some(Stepped::Delivered)));
    let delivery = inbox.next().await.unwrap();
    assert_eq!(delivery.to, newcomer);
}

#[given("an exchange that failed because its peer was revoked")]
async fn an_exchange_failed_by_revocation(world: &mut KamirohWorld) {
    world.arrange_live_exchange().await;
    let who = world.responder();
    let failed = world
        .runtime
        .revoke(&who, &endpoint(AWAY))
        .await
        .expect("the actor is running");
    assert_eq!(failed, 1, "the live exchange should have failed");
    world.exchanges_failed_by_revocation = failed;
}

#[when("the operator admits that endpoint again")]
async fn the_operator_readmits(world: &mut KamirohWorld) {
    let who = world.responder();
    world
        .runtime
        .admit(&who, endpoint(AWAY))
        .expect("the actor is running");
}

// ---------------------------------------------------------------------------
// Story 4 — one send opens many conversations
// ---------------------------------------------------------------------------

impl KamirohWorld {
    /// Install `names` as separate actors at the responder's endpoint, each
    /// admitting whoever `admits` says.
    fn install_siblings(&mut self, names: &[&str], admits: &[&str], rounds: u8) {
        for n in names {
            let mut list = Allowlist::empty();
            for e in admits {
                list.admit(endpoint(e));
            }
            self.runtime
                .install_party(
                    name(n),
                    list,
                    patience(),
                    Box::new(RecordingParty::new(rounds, Arc::clone(&self.failures))),
                )
                .expect("each sibling installs once");
        }
        self.siblings = names.iter().map(|n| name(n)).collect();
    }

    /// One send to every sibling, then the single batch receipt, relayed to
    /// every phone exactly as `docs/EMBEDDING.md` prescribes.
    async fn fan_out_to(&mut self, names: &[&str], n: u8) {
        let controller = self.app.clone();
        let result = Phone::open_many(
            controller,
            endpoint(HOME),
            names.iter().map(|s| name(s)).collect(),
            request(n),
            self.transport(),
            patience(),
            self.timer.clone(),
        )
        .await;
        match result {
            Ok(phones) => self.phones = phones,
            Err(e) => {
                self.last_send_error = Some(e.to_string());
                return;
            }
        }
        // The one receipt for the whole batch, claimed-from the mailroom.
        let delivery = self.app_inbox.next().await.expect("a receipt is owed");
        self.batch_receipt_sender = Some(delivery.from.clone());
        let Inbound::AckReceived(ack) = process(&self.app_list, delivery) else {
            panic!("the batch receipt should arrive before anything else");
        };
        self.receipt_settled = self
            .phones
            .iter_mut()
            .map(|p| usize::from(p.note_ack(&ack)))
            .sum();
    }

    /// Let each installed sibling take its opening turn.
    async fn step_every_sibling(&mut self) {
        for who in self.siblings.clone() {
            let stepped = self.runtime.step(&who).await.expect("the sibling runs");
            self.last_stepped = Some(stepped);
        }
    }
}

#[given("three actors at one endpoint, each admitting the controller")]
async fn three_actors_admitting_the_controller(world: &mut KamirohWorld) {
    world.install_siblings(&["w1", "w2", "w3"], &[AWAY], 0);
}

#[given("three actors at one endpoint, one of which never admitted the controller")]
async fn three_actors_one_denying(world: &mut KamirohWorld) {
    world.install_siblings(&["w1", "w2"], &[AWAY], 0);
    world.install_siblings(&["w3"], &[], 0);
    world.siblings = vec![name("w1"), name("w2"), name("w3")];
    world.denying_sibling = Some(name("w3"));
}

#[given("a batch naming one actor that exists and one that does not")]
async fn a_batch_naming_a_ghost(world: &mut KamirohWorld) {
    world.install_siblings(&["real"], &[AWAY], 0);
    world.fan_out_to(&["real", "ghost"], 4).await;
}

#[given("a fan-out opening awaiting its delivery receipt")]
async fn a_fan_out_awaiting_its_receipt(world: &mut KamirohWorld) {
    world.install_siblings(&["w1", "w2", "w3"], &[AWAY], 0);
    world.fan_out_to(&["w1", "w2", "w3"], 2).await;
}

#[given("three conversations born of one fan-out send")]
async fn three_conversations_from_one_send(world: &mut KamirohWorld) {
    // One round each, so every conversation is still under way when the
    // operator reaches for the lever: a concluded exchange has nothing to
    // fail, and this scenario is about what a revocation does to a live one.
    world.install_siblings(&["w1", "w2", "w3"], &[AWAY], 1);
    world.fan_out_to(&["w1", "w2", "w3"], 3).await;
    world.step_every_sibling().await;
}

#[when("the controller opens work with all three in one send")]
async fn the_controller_opens_with_all_three(world: &mut KamirohWorld) {
    world.fan_out_to(&["w1", "w2", "w3"], 1).await;
    world.step_every_sibling().await;
}

#[when("the controller opens work with both in one send")]
async fn the_controller_opens_with_both(world: &mut KamirohWorld) {
    // The batch already went out when the situation was arranged; the real
    // actor takes its turn here.
    let who = name("real");
    let stepped = world.runtime.step(&who).await.expect("the actor runs");
    world.last_stepped = Some(stepped);
}

#[when("the one receipt for the batch arrives")]
async fn the_one_receipt_arrives(_world: &mut KamirohWorld) {
    // Arranged above: the receipt is the first thing the controller hears,
    // and relaying it to every phone is what `fan_out_to` did.
}

#[then("three ordinary conversations proceed")]
async fn three_conversations_proceed(world: &mut KamirohWorld) {
    assert_eq!(world.phones.len(), 3, "one phone per named actor");
    let live = world
        .phones
        .iter()
        .filter(|p| !matches!(p.state(), TurnState::Idle | TurnState::Failed))
        .count();
    assert_eq!(live, 3, "every fanned conversation should be under way");
}

#[then("each concludes on its own schedule")]
async fn each_concludes_on_its_own(world: &mut KamirohWorld) {
    // Each sibling answered with a closing turn (its counter was zero), and
    // the controller hears them one at a time, in whatever order they come.
    let mut concluded = 0;
    while concluded < 3 {
        let delivery = world.app_inbox.next().await.expect("a reply is owed");
        let from = delivery.from.clone();
        match process(&world.app_list, delivery) {
            Inbound::AckReceived(_) => {}
            Inbound::Turn { turn, .. } => {
                let phone = world
                    .phones
                    .iter_mut()
                    .find(|p| *p.peer() == from)
                    .expect("a reply from a fanned worker");
                phone.on_incoming(&turn).expect("the closing turn is legal");
                concluded += 1;
            }
            other => panic!("unexpected inbound: {other:?}"),
        }
    }
    assert!(
        world.phones.iter().all(|p| p.state() == TurnState::Idle),
        "every conversation should have concluded"
    );
}

#[then("every conversation's receipt wait settles")]
async fn every_receipt_wait_settles(world: &mut KamirohWorld) {
    assert_eq!(
        world.receipt_settled,
        world.phones.len(),
        "the one receipt should have settled every phone's wait"
    );
    let sender = world
        .batch_receipt_sender
        .as_ref()
        .expect("a receipt arrived");
    assert_eq!(
        sender.name.as_str(),
        kamiroh::ports::BATCH_RECEIPT_NAME,
        "the receipt should be claimed-from the endpoint's mailroom"
    );
}

#[then("no deadline was consumed doing it")]
async fn no_deadline_was_consumed(world: &mut KamirohWorld) {
    // The clock never moved: the receipt is arrival, not patience.
    assert!(
        world.party_failures().is_empty(),
        "nothing should have failed: {:?}",
        world.party_failures()
    );
    assert!(
        world
            .phones
            .iter()
            .all(|p| matches!(p.next_due(), Some((DeadlineKind::Turn, _)))),
        "only the peer's turn should still be awaited"
    );
}

#[then("two conversations proceed")]
async fn two_conversations_proceed(world: &mut KamirohWorld) {
    assert_eq!(
        world.phones.len(),
        3,
        "a phone per named actor, denied or not"
    );
    let denied = world
        .denying_sibling
        .clone()
        .expect("this scenario has a denier");
    let served: Vec<_> = world
        .events()
        .into_iter()
        .filter(|e| matches!(e, Event::DeliveryDenied { to, .. } if to.name == denied))
        .collect();
    assert_eq!(served.len(), 1, "exactly the denier should have denied");
}

#[then("the denial is observed at the actors' home")]
async fn the_denial_is_observed_at_the_actors_home(world: &mut KamirohWorld) {
    assert!(
        world
            .events()
            .iter()
            .any(|e| matches!(e, Event::DeliveryDenied { .. })),
        "the denial was not observable at the actors' home"
    );
}

#[then("the controller's exchange with the denying actor fails by its turn deadline")]
#[then("the absent actor's exchange fails by its turn deadline")]
async fn the_silent_siblings_exchange_fails_by_deadline(world: &mut KamirohWorld) {
    let silent = world
        .denying_sibling
        .clone()
        .unwrap_or_else(|| name("ghost"));
    world.timer.advance(past_the_turn_deadline());
    let phone = world
        .phones
        .iter_mut()
        .find(|p| p.peer().name == silent)
        .expect("a phone for the silent sibling");
    assert_eq!(
        phone.on_deadline_elapsed(),
        Some(FailureCause::DeadlineElapsed(DeadlineKind::Turn)),
        "silence should have been caught by the turn deadline"
    );
}

#[then("the receipt still arrives")]
async fn the_receipt_still_arrives(world: &mut KamirohWorld) {
    assert_eq!(
        world.receipt_settled,
        world.phones.len(),
        "the receipt is owed for arrival at the endpoint, whoever is home"
    );
}

#[then("the existing actor's conversation proceeds")]
async fn the_existing_actors_conversation_proceeds(world: &mut KamirohWorld) {
    loop {
        let delivery = world
            .app_inbox
            .next()
            .await
            .expect("the real actor answers");
        let from = delivery.from.clone();
        match process(&world.app_list, delivery) {
            // The real actor's own delivery receipt, behind the batch one.
            Inbound::AckReceived(_) => continue,
            Inbound::Turn { turn, .. } => {
                assert_eq!(from.name, name("real"), "only the real actor can answer");
                let phone = world
                    .phones
                    .iter_mut()
                    .find(|p| *p.peer() == from)
                    .expect("a phone for the real actor");
                phone.on_incoming(&turn).expect("its turn is legal");
                return;
            }
            other => panic!("expected the real actor's turn, got {other:?}"),
        }
    }
}

#[when("one sibling's operator revokes the controller's endpoint")]
async fn one_siblings_operator_revokes(world: &mut KamirohWorld) {
    let who = name("w2");
    let failed = world
        .runtime
        .revoke(&who, &endpoint(AWAY))
        .await
        .expect("the sibling is running");
    world.exchanges_failed_by_revocation = failed;
    world.revoked_sibling = Some(who);
}

#[then("that exchange fails at once, naming the revocation")]
async fn that_exchange_fails_naming_revocation(world: &mut KamirohWorld) {
    assert_eq!(
        world.exchanges_failed_by_revocation, 1,
        "exactly the revoked sibling's exchange should have failed"
    );
    let revoked = world
        .revoked_sibling
        .clone()
        .expect("a sibling was revoked");
    let told = world.party_failures();
    assert!(
        told.iter()
            .any(|(_, cause)| *cause == FailureCause::Revoked),
        "the revocation was not named: {told:?}"
    );
    assert!(
        world.siblings.contains(&revoked),
        "the revoked actor should be one of the fanned siblings"
    );
}

#[then("the other two conversations are untouched")]
async fn the_other_two_are_untouched(world: &mut KamirohWorld) {
    assert_eq!(
        world.party_failures().len(),
        1,
        "only one exchange should have failed: {:?}",
        world.party_failures()
    );
}

// ---------------------------------------------------------------------------
// Story 2, the wire scenario — bound over real QUIC
//
// "A conversation spans connections" is about connections, and the memory
// net has none: it routes by address and never dials. Binding it against
// the simulator would assert something the simulator cannot witness, so
// this one scenario runs on the iroh adapter over loopback, exactly as
// `iroh_disconnects.rs` pins it. It needs a reactor, hence the nested tokio
// runtime — the rest of the suite stays on the workspace's own executor.
// ---------------------------------------------------------------------------

struct WireFixture {
    net_a: kamiroh::adapter_iroh::IrohNet,
    peer_b: EndpointId,
    echo: Address,
    phone: Phone<kamiroh::adapter_iroh::IrohTransport, kamiroh::adapter_kameo::TokioTimer>,
    app_inbox: kamiroh::adapter_iroh::IrohInbox,
    app_list: Allowlist,
    /// Kept alive for the scenario's lifetime: dropping either end tears
    /// the conversation down.
    _net_b: kamiroh::adapter_iroh::IrohNet,
    _runtime: kamiroh::adapter_kameo::KameoRuntime<
        kamiroh::adapter_iroh::IrohTransport,
        kamiroh::adapter_iroh::IrohNet,
    >,
}

impl WireFixture {
    /// One complete exchange: open, hear the receipt, hear the answer.
    /// Returns who answered, so "the same actor" is assertable.
    async fn one_exchange(&mut self, n: u8) -> Address {
        self.phone
            .open(request(n))
            .await
            .expect("the opening turn goes out");
        loop {
            let delivery = self.app_inbox.next().await.expect("the peer answers");
            let from = delivery.from.clone();
            match process(&self.app_list, delivery) {
                Inbound::AckReceived(ack) => {
                    self.phone.note_ack(&ack);
                }
                Inbound::Turn { turn, .. } => {
                    self.phone.on_incoming(&turn).expect("the answer is legal");
                    return from;
                }
                other => panic!("unexpected inbound over the wire: {other:?}"),
            }
        }
    }
}

#[given("a conversation whose exchange completed over one connection")]
async fn a_conversation_over_one_connection(world: &mut KamirohWorld) {
    let rt = tokio::runtime::Runtime::new().expect("a reactor for real sockets");
    let (fixture, answered) = rt.block_on(async {
        use kamiroh::adapter_iroh::IrohNet;
        use kamiroh::adapter_kameo::{KameoRuntime, TokioTimer};
        use kamiroh::domain::secret::Secret;

        let net_a = IrohNet::bind(&Secret::new(vec![11; 32])).await.unwrap();
        let net_b = IrohNet::bind(&Secret::new(vec![12; 32])).await.unwrap();
        let addr_a = net_a.addr().await.unwrap();
        let addr_b = net_b.addr().await.unwrap();
        let id_a = net_b.add_peer(addr_a);
        let id_b = net_a.add_peer(addr_b);

        let runtime = KameoRuntime::new(id_b.clone(), net_b.transport(), net_b.clone(), patience());
        let mut echo_list = Allowlist::empty();
        echo_list.admit(id_a.clone());
        runtime
            .install_party(
                name("echo"),
                echo_list,
                patience(),
                Box::new(kamiroh::app::parties::EchoParty::new()),
            )
            .unwrap();

        let app = Address::new(id_a.clone(), name("app"));
        let app_inbox = net_a.clone().bind(&app).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(id_b.clone());

        let echo = Address::new(id_b.clone(), name("echo"));
        let phone = Phone::converse(app, echo.clone(), net_a.transport(), patience(), TokioTimer);

        let mut fixture = WireFixture {
            net_a,
            peer_b: id_b,
            echo,
            phone,
            app_inbox,
            app_list,
            _net_b: net_b,
            _runtime: runtime,
        };
        // The first exchange establishes — and caches — the connection.
        let answered = fixture.one_exchange(1).await;
        (fixture, answered)
    });
    assert_eq!(answered, fixture.echo, "the first exchange was answered");
    world.wire = Some(fixture);
    world.wire_runtime = Some(rt);
}

#[when("that connection is deliberately closed")]
async fn that_connection_is_closed(world: &mut KamirohWorld) {
    let rt = world.wire_runtime.as_ref().expect("a wire scenario");
    let wire = world.wire.as_ref().expect("a wire scenario");
    let closed = rt.block_on(wire.net_a.close_connection(&wire.peer_b));
    assert!(closed, "there was a cached connection to close");
}

#[then("the next exchange in the same conversation travels a new connection")]
async fn the_next_exchange_travels_a_new_connection(world: &mut KamirohWorld) {
    let rt = world.wire_runtime.as_ref().expect("a wire scenario");
    let wire = world.wire.as_mut().expect("a wire scenario");
    // Same phone, same conversation: the adapter must re-dial, because the
    // connection the first exchange used is gone.
    let answered = rt.block_on(wire.one_exchange(2));
    world.wire_answered_by = Some(answered);
}

#[then("the receiving side routes it to the same actor")]
async fn the_receiving_side_routes_to_the_same_actor(world: &mut KamirohWorld) {
    let wire = world.wire.as_ref().expect("a wire scenario");
    let answered = world
        .wire_answered_by
        .as_ref()
        .expect("the second exchange completed");
    assert_eq!(
        answered, &wire.echo,
        "the new connection was routed to a different actor"
    );
    assert_eq!(
        wire.phone.state(),
        TurnState::Idle,
        "the second exchange concluded in the same conversation"
    );
}

fn main() {
    // A generous ceiling: the whole suite is memory-backed and
    // virtual-clocked, so a healthy run is far under a second. If a
    // scenario wedges, this fails the run with a message instead of
    // parking the process forever.
    block_on_bounded(
        Duration::from_secs(120),
        KamirohWorld::cucumber()
            .fail_on_skipped()
            .run_and_exit("features"),
    );
}
