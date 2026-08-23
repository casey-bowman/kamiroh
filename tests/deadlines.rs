//! Story 1 of the workshop-2 example-mapping session, pinned by test: a hung
//! exchange fails loudly (`ARCHITECTURE.md`, decisions 22–25; the co-authored
//! scenarios live in `features/`).
//!
//! All timing is virtual — the `TestTimer` moves only when a test says so, so
//! sixty-second deadlines elapse in microseconds, deterministically.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kamiroh::adapter_memory::MemoryNet;
use kamiroh::adapter_memory::testing::{TestTimer, block_on};
use kamiroh::app::inbound::{Inbound, process};
use kamiroh::app::phone::{Phone, PhoneError};
use kamiroh::app::runtime::{LocalRuntime, Stepped};
use kamiroh::app::wait::{Waited, next_or_deadline};
use kamiroh::domain::actor::{ActorName, Address};
use kamiroh::domain::allowlist::Allowlist;
use kamiroh::domain::deadline::{DeadlineKind, Deadlines, FailureCause};
use kamiroh::domain::endpoint::EndpointId;
use kamiroh::domain::hex::Hex;
use kamiroh::domain::protocol::{TurnError, TurnState};
use kamiroh::domain::vocabulary::{Ack, Message, Request, RequestId, Response, Turn};
use kamiroh::ports::{Event, Inbox as _, Party, Timer as _, Transport as _};

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

/// Five seconds of patience for delivery receipts, sixty for thinking.
fn patience() -> Deadlines {
    Deadlines::new(Duration::from_secs(5), Duration::from_secs(60))
}

/// Scenario: an ack that never comes.
///
/// The peer is registered (sends reach a mailbox) but nothing ever processes
/// them — the receipt never arrives, and the ack deadline fails the exchange.
#[test]
fn an_ack_that_never_comes_fails_the_exchange() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let silent = address("bb", "silent");
        let _their_inbox = net.register(silent.clone()).unwrap();

        let timer = TestTimer::new();
        let mut phone = Phone::converse(
            app.clone(),
            silent,
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();

        // The earliest wait is the ack (5s < 60s).
        let (kind, due) = phone.next_due().expect("waits should be armed");
        assert_eq!(kind, DeadlineKind::Ack);

        // Six virtual seconds of silence.
        timer.advance(Duration::from_secs(6));
        let waited = next_or_deadline(&mut app_inbox, phone.timer().sleep_until(due)).await;
        assert!(matches!(waited, Waited::DeadlineElapsed));

        let cause = phone.on_deadline_elapsed().expect("a wait was pending");
        assert_eq!(cause, FailureCause::DeadlineElapsed(DeadlineKind::Ack));
        assert_eq!(phone.state(), TurnState::Failed);
    });
}

/// Scenarios: a peer that never answers; a late turn after a failed exchange.
///
/// The peer's runtime acks (delivery confirmed) but the party's answer never
/// comes: the turn deadline fails the exchange. The answer that finally
/// arrives is refused, and a fresh exchange may open in the same
/// conversation.
#[test]
fn a_peer_that_never_answers_fails_the_exchange_and_late_turns_are_refused() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));
        let peer = address("bb", "slow");
        let _their_inbox = net.register(peer.clone()).unwrap();

        let timer = TestTimer::new();
        let mut phone = Phone::converse(
            app.clone(),
            peer.clone(),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();

        // Their runtime acks on handover (simulated by hand: the memory
        // transport accepts a claimed origin).
        net.transport()
            .send(&peer, &app, Message::Ack(Ack { id: request(1).id }))
            .await
            .unwrap();
        match process(&app_list, app_inbox.next().await.unwrap()) {
            Inbound::AckReceived(ack) => assert!(phone.note_ack(&ack)),
            other => panic!("expected the ack, got {other:?}"),
        }

        // Delivery is confirmed; the only wait left is their turn (60s).
        let (kind, due) = phone.next_due().expect("turn wait should remain");
        assert_eq!(kind, DeadlineKind::Turn);

        // Sixty-one virtual seconds of thinking that never ends.
        timer.advance(Duration::from_secs(61));
        let waited = next_or_deadline(&mut app_inbox, phone.timer().sleep_until(due)).await;
        assert!(matches!(waited, Waited::DeadlineElapsed));
        let cause = phone.on_deadline_elapsed().expect("a wait was pending");
        assert_eq!(cause, FailureCause::DeadlineElapsed(DeadlineKind::Turn));

        // The peer's genuine answer finally arrives — to a failed exchange.
        let late = Turn::Close {
            response: Response {
                id: request(1).id,
                body: vec![1],
            },
        };
        assert_eq!(
            phone.on_incoming(&late),
            Err(PhoneError::Turn(TurnError::ExchangeFailed))
        );

        // The conversation survives: a fresh exchange opens.
        phone.open(request(2)).await.unwrap();
        assert!(matches!(phone.state(), TurnState::AwaitingTheirTurn { .. }));
    });
}

/// A party that answers an `Open` and poses a question of its own, and
/// records what it is told about failures.
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

/// Scenarios: the two sides converge on failure separately; the party is
/// told; the observer sees the event.
///
/// A responder party poses a question whose asker vanishes: the runtime's
/// deadline fails the exchange on the responder side — with no failure
/// message on the wire — and both the party and the observer hear about it.
#[test]
fn a_responders_unanswered_question_fails_loudly_on_its_own_clock() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let timer = TestTimer::new();
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
        let mut asker_list = Allowlist::empty();
        asker_list.admit(endpoint("aa"));
        runtime
            .install_party(
                name("asker"),
                asker_list,
                patience(),
                Box::new(AskingParty {
                    failures: Arc::clone(&failures),
                }),
            )
            .unwrap();
        let asker = address("bb", "asker");

        // The app opens; the party answers and poses its own question…
        let mut phone = Phone::converse(
            app.clone(),
            asker.clone(),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();
        assert!(matches!(
            runtime.step(&name("asker")).await.unwrap(),
            Stepped::Delivered
        ));
        // …which reaches the app (ack, then the Continue), and the app
        // then falls silent forever.
        loop {
            match process(&app_list, app_inbox.next().await.unwrap()) {
                Inbound::AckReceived(ack) => {
                    phone.note_ack(&ack);
                }
                Inbound::Turn { turn, .. } => {
                    phone.on_incoming(&turn).unwrap();
                    break;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }

        // The responder's ack deadline (its own clock, its own patience)
        // elapses first: no receipt for the question it posed ever came.
        timer.advance(Duration::from_secs(6));
        let stepped = runtime.step(&name("asker")).await.unwrap();
        let expected_cause = FailureCause::DeadlineElapsed(DeadlineKind::Ack);
        assert_eq!(
            stepped,
            Stepped::ExchangeFailed {
                peer: app.clone(),
                cause: expected_cause,
            }
        );

        // The party was told (decision 25: application semantics)…
        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[(app.clone(), expected_cause)]
        );
        // …and the observer saw it (observability), locally only.
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[Event::ExchangeFailed {
                local: asker.clone(),
                peer: app.clone(),
                cause: expected_cause,
            }]
        );
    });
}

/// Scenario: a slow but timely answer — time passes, but within patience;
/// nothing fails, and the settled waits leave nothing armed.
#[test]
fn a_timely_answer_does_not_fail() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let timer = TestTimer::new();
        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            net.transport(),
            net.clone(),
            timer.clone(),
            patience(),
        );
        let mut echo_list = Allowlist::empty();
        echo_list.admit(endpoint("aa"));
        runtime
            .install_party(
                name("echo"),
                echo_list,
                patience(),
                Box::new(kamiroh::app::parties::EchoParty::new()),
            )
            .unwrap();

        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "echo"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(7)).await.unwrap();

        // Three virtual seconds pass — within every deadline.
        timer.advance(Duration::from_secs(3));
        runtime.step(&name("echo")).await.unwrap();

        let mut concluded = false;
        while !concluded {
            match process(&app_list, app_inbox.next().await.unwrap()) {
                Inbound::AckReceived(ack) => {
                    assert!(phone.note_ack(&ack));
                }
                Inbound::Turn { turn, .. } => {
                    phone.on_incoming(&turn).unwrap();
                    concluded = true;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
        assert_eq!(phone.state(), TurnState::Idle);
        assert!(phone.next_due().is_none(), "no wait should remain armed");
    });
}

/// Deny observability (roadmap item 4, decision 25): a denied delivery and a
/// dropped illegal turn are silent on the wire but assertable at home.
#[test]
fn denials_and_illegal_turns_are_observable_locally() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
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

        // bob admits only aa.
        let mut bob_list = Allowlist::empty();
        bob_list.admit(endpoint("aa"));
        runtime
            .install(
                name("bob"),
                bob_list,
                patience(),
                kamiroh::app::runtime::ActorKind::Plain,
            )
            .unwrap();
        let bob = address("bb", "bob");

        // An unadmitted endpoint sends: denied, silently — but observably.
        let mallory = address("cc", "mallory");
        net.transport()
            .send(&mallory, &bob, Message::Request(request(1)))
            .await
            .unwrap();
        runtime.step(&name("bob")).await.unwrap();

        // An admitted endpoint sends an out-of-nowhere Close: an illegal
        // turn, dropped — but observably.
        let alice = address("aa", "alice");
        net.transport()
            .send(
                &alice,
                &bob,
                Message::Turn(Turn::Close {
                    response: Response {
                        id: RequestId([9; 16]),
                        body: vec![],
                    },
                }),
            )
            .await
            .unwrap();
        runtime.step(&name("bob")).await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[
                Event::DeliveryDenied {
                    from: mallory,
                    to: bob.clone(),
                },
                Event::IllegalTurnDropped {
                    from: alice,
                    to: bob,
                    error: TurnError::NoExchange,
                },
            ]
        );
    });
}

/// A party that answers the wrong request — an illegal reply.
struct MisansweringParty;

impl Party for MisansweringParty {
    async fn on_turn(&mut self, _from: &Address, _turn: Turn) -> Option<Turn> {
        Some(Turn::Close {
            response: Response {
                id: RequestId([250; 16]), // answers nothing outstanding
                body: vec![],
            },
        })
    }
}

/// Decision 26: a party's illegal reply is dropped and the exchange fails
/// loudly — never a silently un-deadlined hang.
#[test]
fn an_illegal_party_reply_fails_the_exchange_loudly() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let _app_inbox = net.register(app.clone()).unwrap();

        let timer = TestTimer::new();
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

        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        runtime
            .install_party(name("wrong"), list, patience(), Box::new(MisansweringParty))
            .unwrap();
        let wrong = address("bb", "wrong");

        let mut phone = Phone::converse(
            app.clone(),
            wrong.clone(),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();

        let stepped = runtime.step(&name("wrong")).await.unwrap();
        let cause = FailureCause::IllegalReply;
        assert_eq!(
            stepped,
            Stepped::ExchangeFailed {
                peer: app.clone(),
                cause,
            }
        );
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[Event::ExchangeFailed {
                local: wrong,
                peer: app,
                cause,
            }]
        );
    });
}

/// A transport that refuses to carry turns (or, separately, acks) on
/// command; everything else passes through to the memory net.
#[derive(Clone)]
struct TurnRefusingTransport {
    inner: kamiroh::adapter_memory::MemoryTransport,
    refuse_turns: Arc<std::sync::atomic::AtomicBool>,
    refuse_acks: Arc<std::sync::atomic::AtomicBool>,
}

impl kamiroh::ports::Transport for TurnRefusingTransport {
    type Error = std::io::Error;

    async fn send(
        &mut self,
        from: &Address,
        to: &Address,
        message: Message,
    ) -> Result<(), Self::Error> {
        if matches!(message, Message::Turn(_))
            && self.refuse_turns.load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(std::io::Error::other("turn refused by test transport"));
        }
        if matches!(message, Message::Ack(_))
            && self.refuse_acks.load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(std::io::Error::other("ack refused by test transport"));
        }
        self.inner
            .send(from, to, message)
            .await
            .map_err(std::io::Error::other)
    }

    async fn open_many(
        &mut self,
        from: &Address,
        to_endpoint: &kamiroh::domain::endpoint::EndpointId,
        to_names: &[kamiroh::domain::actor::ActorName],
        request: kamiroh::domain::vocabulary::Request,
    ) -> Result<(), Self::Error> {
        // A batch opening is turns on the wire: refused with the rest.
        if self.refuse_turns.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(std::io::Error::other("batch refused by test transport"));
        }
        self.inner
            .open_many(from, to_endpoint, to_names, request)
            .await
            .map_err(std::io::Error::other)
    }
}

/// Decision 26 meets decision 29: a refused batch send fails loudly at
/// once — the error returns and no phones exist, so nothing was armed
/// against deadlines the transport already answered. (The review's ask:
/// the doc promised this, and only this test makes a
/// build-phones-before-sending refactor unable to sneak past it.)
#[test]
fn a_refused_batch_creates_no_phones() {
    block_on(async {
        let net = MemoryNet::new();
        let controller = address("aa", "controller");
        let _controller_inbox = net.register(controller.clone()).unwrap();

        let refuse = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let transport = TurnRefusingTransport {
            inner: net.transport(),
            refuse_turns: Arc::clone(&refuse),
            refuse_acks: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let timer = TestTimer::new();
        let result = Phone::open_many(
            controller,
            endpoint("bb"),
            vec![name("w1"), name("w2")],
            request(1),
            transport,
            patience(),
            timer,
        )
        .await;
        assert!(matches!(result, Err(PhoneError::Transport(_))));
    });
}

/// Decision 26: when the transport refuses a responder's reply turn, the
/// exchange fails at once — party told, observer sees it — instead of
/// waiting out waits for a turn that never left.
#[test]
fn a_refused_reply_send_fails_the_exchange_at_once() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let refuse_turns = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let transport = TurnRefusingTransport {
            inner: net.transport(),
            refuse_turns: Arc::clone(&refuse_turns),
            refuse_acks: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let timer = TestTimer::new();
        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            transport,
            net.clone(),
            timer.clone(),
            patience(),
        );
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        runtime.set_observer(move |event: Event| sink.lock().unwrap().push(event));

        let failures = Arc::new(Mutex::new(Vec::new()));
        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        runtime
            .install_party(
                name("asker"),
                list,
                patience(),
                Box::new(AskingParty {
                    failures: Arc::clone(&failures),
                }),
            )
            .unwrap();
        let asker = address("bb", "asker");

        let mut phone = Phone::converse(
            app.clone(),
            asker.clone(),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();

        // The ack goes through; the party's reply turn is refused.
        refuse_turns.store(true, std::sync::atomic::Ordering::SeqCst);
        let stepped = runtime.step(&name("asker")).await.unwrap();
        let cause = FailureCause::SendFailed;
        assert_eq!(
            stepped,
            Stepped::ExchangeFailed {
                peer: app.clone(),
                cause,
            }
        );
        assert_eq!(failures.lock().unwrap().as_slice(), &[(app.clone(), cause)]);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[Event::ExchangeFailed {
                local: asker,
                peer: app,
                cause,
            }]
        );
        // The ack did arrive — only the turn was refused.
        let delivery = app_inbox.next().await.unwrap();
        assert!(matches!(
            process(&app_list, delivery),
            Inbound::AckReceived(_)
        ));
    });
}

/// External-review pin (2026-08-23, ruled by Casey): a refused handover
/// ack does NOT wedge the exchange. The ack is ignored, the party still
/// runs, and the verdict comes loudly from the reply send — here the peer
/// vanished entirely, so the reply is refused and the exchange fails with
/// `SendFailed` through both surfaces. (Previously the step aborted after
/// the machine had moved: party never called, no deadline armed — a
/// silent wedge in the runtime documented as the readable reference.)
#[test]
fn a_refused_handover_ack_does_not_wedge_the_exchange() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let _app_inbox = net.register(app.clone()).unwrap();

        let timer = TestTimer::new();
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
        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        runtime
            .install_party(
                name("asker"),
                list,
                patience(),
                Box::new(AskingParty {
                    failures: Arc::clone(&failures),
                }),
            )
            .unwrap();

        // The Open is delivered, then the sender's endpoint vanishes —
        // so BOTH the handover ack and the party's reply will be refused.
        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "asker"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();
        net.kill_endpoint(&endpoint("aa"));

        // One step: ack refused (ignored), party consulted, reply refused
        // — the exchange fails loudly, not silently.
        let stepped = runtime.step(&name("asker")).await.unwrap();
        assert_eq!(
            stepped,
            Stepped::ExchangeFailed {
                peer: app.clone(),
                cause: FailureCause::SendFailed,
            }
        );
        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[(app.clone(), FailureCause::SendFailed)]
        );
        assert!(events.lock().unwrap().iter().any(|e| matches!(
            e,
            Event::ExchangeFailed { peer, cause: FailureCause::SendFailed, .. } if *peer == app
        )));
    });
}

/// The graceful half of the same ruling: when only the ACK send blips,
/// the exchange continues — the party's reply goes through, and the
/// arriving turn proves delivery to the initiator (a turn supersedes a
/// missing ack), so nothing failed and nothing hung.
#[test]
fn a_blipped_ack_alone_interrupts_nothing() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let refuse_acks = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let transport = TurnRefusingTransport {
            inner: net.transport(),
            refuse_turns: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            refuse_acks: Arc::clone(&refuse_acks),
        };

        let timer = TestTimer::new();
        let mut runtime = LocalRuntime::new(
            endpoint("bb"),
            transport,
            net.clone(),
            timer.clone(),
            patience(),
        );
        let failures = Arc::new(Mutex::new(Vec::new()));
        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        runtime
            .install_party(
                name("asker"),
                list,
                patience(),
                Box::new(AskingParty {
                    failures: Arc::clone(&failures),
                }),
            )
            .unwrap();

        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "asker"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();

        // The ack blips; the reply does not. The exchange continues.
        assert!(matches!(
            runtime.step(&name("asker")).await.unwrap(),
            Stepped::Delivered
        ));
        assert!(failures.lock().unwrap().is_empty());
        let delivery = app_inbox.next().await.unwrap();
        match process(&app_list, delivery) {
            Inbound::Turn { turn, .. } => {
                // The turn itself proves delivery: on_incoming settles the
                // still-missing ack wait.
                phone.on_incoming(&turn).unwrap();
                // Every wait settled — the phone now owes its own turn.
                assert!(phone.next_due().is_none());
            }
            other => panic!("expected the party's reply, got {other:?}"),
        }
    });
}
