//! Story 4 of workshop-2, pinned by test: one send opens many
//! conversations (`ARCHITECTURE.md`, decision 29; scenarios in
//! `features/`). Deterministic: the memory net carries the batch, and
//! time is the hand-cranked `TestTimer` — it never moves except where a
//! scenario is explicitly about a deadline.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kamiroh::adapter_memory::MemoryNet;
use kamiroh::adapter_memory::testing::{TestTimer, block_on};
use kamiroh::app::inbound::{Inbound, process};
use kamiroh::app::parties::EchoParty;
use kamiroh::app::phone::Phone;
use kamiroh::app::runtime::{LocalRuntime, Stepped};
use kamiroh::domain::actor::{ActorName, Address};
use kamiroh::domain::allowlist::Allowlist;
use kamiroh::domain::deadline::{DeadlineKind, Deadlines, FailureCause};
use kamiroh::domain::endpoint::EndpointId;
use kamiroh::domain::hex::Hex;
use kamiroh::domain::protocol::{TurnProgress, TurnState};
use kamiroh::domain::vocabulary::{Request, RequestId, Response, Turn};
use kamiroh::ports::{BATCH_RECEIPT_NAME, Event, Inbox as _, Party};

fn endpoint(s: &str) -> EndpointId {
    EndpointId::new(Hex::new(s).unwrap())
}

fn name(s: &str) -> ActorName {
    ActorName::new(s).unwrap()
}

fn address(e: &str, n: &str) -> Address {
    Address::new(endpoint(e), name(n))
}

fn request(n: u8) -> Request {
    Request {
        id: RequestId([n; 16]),
        body: vec![n],
    }
}

fn patience() -> Deadlines {
    Deadlines::new(Duration::from_secs(5), Duration::from_secs(60))
}

/// A party that answers an `Open` with a question of its own and records
/// failure notifications — keeps the exchange live.
struct AskingParty {
    failures: Arc<Mutex<Vec<(Address, FailureCause)>>>,
}

impl Party for AskingParty {
    async fn on_turn(&mut self, _from: &Address, turn: Turn) -> Option<Turn> {
        match turn {
            Turn::Open { request } => Some(Turn::Continue {
                response: Response {
                    id: request.id,
                    body: request.body,
                },
                request: Request {
                    id: RequestId([200; 16]),
                    body: vec![],
                },
            }),
            _ => None,
        }
    }

    fn on_exchange_failed(
        &mut self,
        from: &Address,
        cause: FailureCause,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.failures.lock().unwrap().push((from.clone(), cause));
        async {}
    }
}

/// The controller side of every test: fan out to `names` at `bb`, then
/// relay the batch receipt to every phone (the endpoint-scoped relay rule
/// — see EMBEDDING) and assert it settled each ack wait.
async fn fan_out_and_settle(
    net: &MemoryNet,
    timer: &TestTimer,
    controller: &Address,
    controller_inbox: &mut <MemoryNet as kamiroh::ports::Registry>::Inbox,
    controller_list: &Allowlist,
    names: &[&str],
    n: u8,
) -> Vec<Phone<kamiroh::adapter_memory::MemoryTransport, TestTimer>> {
    let mut phones = Phone::open_many(
        controller.clone(),
        endpoint("bb"),
        names.iter().map(|s| name(s)).collect(),
        request(n),
        net.transport(),
        patience(),
        timer.clone(),
    )
    .await
    .unwrap();
    assert_eq!(phones.len(), names.len());

    // One receipt for the whole batch, claimed-from the mailroom.
    let delivery = controller_inbox.next().await.unwrap();
    assert_eq!(delivery.from.endpoint, endpoint("bb"));
    assert_eq!(delivery.from.name.as_str(), BATCH_RECEIPT_NAME);
    let Inbound::AckReceived(ack) = process(controller_list, delivery) else {
        panic!("expected the batch receipt first");
    };
    for phone in &mut phones {
        assert!(phone.note_ack(&ack), "the one receipt settles every wait");
        // Only the turn wait remains.
        assert_eq!(phone.next_due().unwrap().0, DeadlineKind::Turn);
    }
    phones
}

/// Admit the mailroom's endpoint (bb) and the workers at bb — the
/// controller's own inbound policy.
fn controller_list() -> Allowlist {
    let mut list = Allowlist::empty();
    list.admit(endpoint("bb"));
    list
}

/// Scenario: one send reaches three actors — three ordinary
/// conversations, each concluding on its own schedule. Also pins the
/// accepted residue: each admitted worker still acks pairwise; those
/// later acks settle nothing (note_ack is idempotent).
#[test]
fn one_send_opens_three_conversations() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let controller = address("aa", "controller");
        let mut controller_inbox = net.register(controller.clone()).unwrap();
        let list = controller_list();

        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            net.transport(),
            net.clone(),
            timer.clone(),
            patience(),
        );
        for w in ["w1", "w2", "w3"] {
            let mut allow = Allowlist::empty();
            allow.admit(endpoint("aa"));
            runtime
                .install_party(name(w), allow, patience(), Box::new(EchoParty::new()))
                .unwrap();
        }

        let mut phones = fan_out_and_settle(
            &net,
            &timer,
            &controller,
            &mut controller_inbox,
            &list,
            &["w1", "w2", "w3"],
            1,
        )
        .await;

        // Each worker processes its Open on its own schedule (echoes a
        // Close); the controller routes each reply to its phone by the
        // proven sender.
        for w in ["w1", "w2", "w3"] {
            runtime.step(&name(w)).await.unwrap();
        }
        let mut concluded = 0;
        while concluded < 3 {
            let delivery = controller_inbox.next().await.unwrap();
            let from = delivery.from.clone();
            match process(&list, delivery) {
                Inbound::AckReceived(ack) => {
                    // The workers' own pairwise acks: already settled by
                    // the batch receipt — they change nothing.
                    for phone in &mut phones {
                        assert!(!phone.note_ack(&ack));
                    }
                }
                Inbound::Turn { turn, .. } => {
                    let phone = phones
                        .iter_mut()
                        .find(|p| *p.peer() == from)
                        .expect("a reply from a fanned worker");
                    assert_eq!(phone.on_incoming(&turn).unwrap(), TurnProgress::Concluded);
                    concluded += 1;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
        for phone in &phones {
            assert_eq!(phone.state(), TurnState::Idle);
        }
    });
}

/// Scenario: the batch receipt settles every wait at once — and no
/// deadline was consumed doing it (the timer never moves).
#[test]
fn the_batch_receipt_settles_every_ack_wait_at_once() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let controller = address("aa", "controller");
        let mut controller_inbox = net.register(controller.clone()).unwrap();
        let list = controller_list();

        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            net.transport(),
            net.clone(),
            timer.clone(),
            patience(),
        );
        for w in ["w1", "w2"] {
            let mut allow = Allowlist::empty();
            allow.admit(endpoint("aa"));
            runtime
                .install_party(name(w), allow, patience(), Box::new(EchoParty::new()))
                .unwrap();
        }

        // fan_out_and_settle asserts the settling itself; this scenario is
        // that assertion, plus the untouched clock.
        let phones = fan_out_and_settle(
            &net,
            &timer,
            &controller,
            &mut controller_inbox,
            &list,
            &["w1", "w2"],
            2,
        )
        .await;
        drop(phones);
    });
}

/// Scenario: a denied sibling is silence, and the rest proceed. The
/// denial is observed at the workers' home; the controller's exchange
/// with the denier fails by its turn deadline — the fanned conversation
/// traded its short-deadline safety net away (decision 29's documented
/// residue).
#[test]
fn a_denied_sibling_is_silence_and_the_rest_proceed() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let controller = address("aa", "controller");
        let mut controller_inbox = net.register(controller.clone()).unwrap();
        let list = controller_list();

        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            net.transport(),
            net.clone(),
            timer.clone(),
            patience(),
        );
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        runtime.set_observer(move |event: Event| sink.lock().unwrap().push(event));
        // w1 and w3 admit the controller; w2 never did.
        for (w, admits) in [("w1", true), ("w2", false), ("w3", true)] {
            let mut allow = Allowlist::empty();
            if admits {
                allow.admit(endpoint("aa"));
            }
            runtime
                .install_party(name(w), allow, patience(), Box::new(EchoParty::new()))
                .unwrap();
        }

        let mut phones = fan_out_and_settle(
            &net,
            &timer,
            &controller,
            &mut controller_inbox,
            &list,
            &["w1", "w2", "w3"],
            3,
        )
        .await;

        for w in ["w1", "w2", "w3"] {
            runtime.step(&name(w)).await.unwrap();
        }
        // The denial is observed at the workers' home.
        assert!(events.lock().unwrap().iter().any(|e| matches!(
            e,
            Event::DeliveryDenied { from, to } if *from == controller && to.name == name("w2")
        )));

        // The two admitted conversations conclude...
        let mut concluded = 0;
        while concluded < 2 {
            let delivery = controller_inbox.next().await.unwrap();
            let from = delivery.from.clone();
            match process(&list, delivery) {
                Inbound::AckReceived(_) => {}
                Inbound::Turn { turn, .. } => {
                    let phone = phones.iter_mut().find(|p| *p.peer() == from).unwrap();
                    assert_eq!(phone.on_incoming(&turn).unwrap(), TurnProgress::Concluded);
                    concluded += 1;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }

        // ...while w2's phone hears nothing at all. Silence is the turn
        // deadline's job: advance past it and fail the exchange.
        let w2_phone = phones
            .iter_mut()
            .find(|p| p.peer().name == name("w2"))
            .unwrap();
        let (kind, _) = w2_phone.next_due().unwrap();
        assert_eq!(kind, DeadlineKind::Turn);
        timer.advance(Duration::from_secs(61));
        assert_eq!(
            w2_phone.on_deadline_elapsed(),
            Some(FailureCause::DeadlineElapsed(DeadlineKind::Turn))
        );
        assert_eq!(w2_phone.state(), TurnState::Failed);
    });
}

/// Scenario: an absent sibling discloses nothing — the receipt still
/// arrives (even for a name that does not exist), the bound sibling's
/// conversation proceeds, and the absent one is ordinary silence. Also
/// pins the all-unbound half: a batch naming nobody real still gets its
/// receipt, because the receipt promises arrival, not existence.
#[test]
fn an_absent_sibling_discloses_nothing() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let controller = address("aa", "controller");
        let mut controller_inbox = net.register(controller.clone()).unwrap();
        let list = controller_list();

        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            net.transport(),
            net.clone(),
            timer.clone(),
            patience(),
        );
        let mut allow = Allowlist::empty();
        allow.admit(endpoint("aa"));
        runtime
            .install_party(name("real"), allow, patience(), Box::new(EchoParty::new()))
            .unwrap();

        // One real name, one ghost: the receipt arrives and settles both.
        let mut phones = fan_out_and_settle(
            &net,
            &timer,
            &controller,
            &mut controller_inbox,
            &list,
            &["real", "ghost"],
            4,
        )
        .await;

        runtime.step(&name("real")).await.unwrap();
        let mut real_done = false;
        while !real_done {
            let delivery = controller_inbox.next().await.unwrap();
            let from = delivery.from.clone();
            match process(&list, delivery) {
                Inbound::AckReceived(_) => {}
                Inbound::Turn { turn, .. } => {
                    assert_eq!(from.name, name("real"));
                    let phone = phones.iter_mut().find(|p| *p.peer() == from).unwrap();
                    phone.on_incoming(&turn).unwrap();
                    real_done = true;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
        // The ghost is silence, caught by the turn deadline like every
        // silence.
        let ghost_phone = phones
            .iter_mut()
            .find(|p| p.peer().name == name("ghost"))
            .unwrap();
        timer.advance(Duration::from_secs(61));
        assert_eq!(
            ghost_phone.on_deadline_elapsed(),
            Some(FailureCause::DeadlineElapsed(DeadlineKind::Turn))
        );

        // The all-unbound half: nobody real at all — the receipt still
        // arrives (fan_out_and_settle asserts it).
        let phones = fan_out_and_settle(
            &net,
            &timer,
            &controller,
            &mut controller_inbox,
            &list,
            &["ghost1", "ghost2"],
            5,
        )
        .await;
        drop(phones);
    });
}

/// External-review pin (2026-08-23): a batch to an endpoint that does not
/// exist is refused — no receipt, no phones. The receipt promises "the
/// batch reached the endpoint," and on the wire that requires a
/// successful connect; the simulator's honest analogue is at least one
/// mailbox at the target endpoint. (A LIVE endpoint whose named actors
/// don't exist still acks — pinned by an_absent_sibling_discloses_nothing.)
#[test]
fn a_batch_to_an_absent_endpoint_is_refused() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let controller = address("aa", "controller");
        let _controller_inbox = net.register(controller.clone()).unwrap();

        // Nothing lives at cc.
        let result = Phone::open_many(
            controller,
            endpoint("cc"),
            vec![name("w1"), name("w2")],
            request(7),
            net.transport(),
            patience(),
            timer,
        )
        .await;
        assert!(matches!(
            result,
            Err(kamiroh::app::phone::PhoneError::Transport(_))
        ));
    });
}

/// Scenario: a fanned conversation can be revoked alone (decision 28
/// interplay) — nothing about fan-out changes mutation, or vice versa.
#[test]
fn a_fanned_conversation_can_be_revoked_alone() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let controller = address("aa", "controller");
        let mut controller_inbox = net.register(controller.clone()).unwrap();
        let list = controller_list();

        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            net.transport(),
            net.clone(),
            timer.clone(),
            patience(),
        );
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        runtime.set_observer(move |event: Event| sink.lock().unwrap().push(event));
        let failures = Arc::new(Mutex::new(Vec::new()));
        for w in ["w1", "w2", "w3"] {
            let mut allow = Allowlist::empty();
            allow.admit(endpoint("aa"));
            runtime
                .install_party(
                    name(w),
                    allow,
                    patience(),
                    Box::new(AskingParty {
                        failures: Arc::clone(&failures),
                    }),
                )
                .unwrap();
        }

        let mut phones = fan_out_and_settle(
            &net,
            &timer,
            &controller,
            &mut controller_inbox,
            &list,
            &["w1", "w2", "w3"],
            6,
        )
        .await;

        // All three exchanges go live (each worker continues with its own
        // question).
        for w in ["w1", "w2", "w3"] {
            runtime.step(&name(w)).await.unwrap();
        }
        let mut heard = 0;
        while heard < 3 {
            let delivery = controller_inbox.next().await.unwrap();
            let from = delivery.from.clone();
            match process(&list, delivery) {
                Inbound::AckReceived(_) => {}
                Inbound::Turn { turn, .. } => {
                    let phone = phones.iter_mut().find(|p| *p.peer() == from).unwrap();
                    phone.on_incoming(&turn).unwrap();
                    heard += 1;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }

        // w2's operator revokes the controller: exactly that exchange
        // fails, at once, naming the revocation.
        let failed = runtime.revoke(&name("w2"), &endpoint("aa")).await.unwrap();
        assert_eq!(failed, 1);
        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[(controller.clone(), FailureCause::Revoked)]
        );

        // The other two conversations are untouched: their closing turns
        // are delivered.
        for w in ["w1", "w3"] {
            let phone = phones
                .iter_mut()
                .find(|p| p.peer().name == name(w))
                .unwrap();
            phone
                .send_turn(Turn::Close {
                    response: Response {
                        id: RequestId([200; 16]),
                        body: vec![],
                    },
                })
                .await
                .unwrap();
            assert!(matches!(
                runtime.step(&name(w)).await.unwrap(),
                Stepped::Delivered
            ));
        }
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, Event::ExchangeFailed { .. }))
                .count(),
            1
        );
    });
}
