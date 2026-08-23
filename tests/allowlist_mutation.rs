//! Story 3 of workshop-2, pinned by test: an actor's guest list changes
//! while it runs (`ARCHITECTURE.md`, decision 28; scenarios in `features/`).
//! Deterministic: no fault levers needed — revocation is a runtime API
//! call, not a transport fault — and time is the hand-cranked `TestTimer`,
//! which never moves in the positive-evidence tests.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kamiroh::adapter_memory::MemoryNet;
use kamiroh::adapter_memory::testing::{TestTimer, block_on};
use kamiroh::app::inbound::{Inbound, process};
use kamiroh::app::phone::Phone;
use kamiroh::app::runtime::{ActorKind, LocalRuntime, Stepped};
use kamiroh::domain::actor::{ActorName, Address};
use kamiroh::domain::allowlist::Allowlist;
use kamiroh::domain::deadline::{Deadlines, FailureCause};
use kamiroh::domain::endpoint::EndpointId;
use kamiroh::domain::hex::Hex;
use kamiroh::domain::protocol::TurnState;
use kamiroh::domain::vocabulary::{Harness, Message, Request, RequestId, Response, Turn};
use kamiroh::ports::{Event, Inbox as _, Party, Transport as _};

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
/// failure notifications.
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

/// The shared stage: a runtime at `bb` hosting an `AskingParty` actor whose
/// allowlist starts with exactly `admitted`, an observer recording events,
/// and the party's failure log.
#[allow(clippy::type_complexity)]
fn stage(
    net: &MemoryNet,
    timer: &TestTimer,
    admitted: &[&str],
) -> (
    LocalRuntime<kamiroh::adapter_memory::MemoryTransport, MemoryNet, TestTimer>,
    Arc<Mutex<Vec<Event>>>,
    Arc<Mutex<Vec<(Address, FailureCause)>>>,
) {
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
    for e in admitted {
        list.admit(endpoint(e));
    }
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
    (runtime, events, failures)
}

/// Open an exchange from `app` and drive it until both sides are live: the
/// party has continued with its own question, the phone has heard it.
async fn open_live_exchange(
    net: &MemoryNet,
    timer: &TestTimer,
    runtime: &mut LocalRuntime<kamiroh::adapter_memory::MemoryTransport, MemoryNet, TestTimer>,
    app: &Address,
    app_list: &Allowlist,
    app_inbox: &mut <MemoryNet as kamiroh::ports::Registry>::Inbox,
    n: u8,
) -> Phone<kamiroh::adapter_memory::MemoryTransport, TestTimer> {
    let mut phone = Phone::converse(
        app.clone(),
        address("bb", "asker"),
        net.transport(),
        patience(),
        timer.clone(),
    );
    phone.open(request(n)).await.unwrap();
    assert!(matches!(
        runtime.step(&name("asker")).await.unwrap(),
        Stepped::Delivered
    ));
    loop {
        match process(app_list, app_inbox.next().await.unwrap()) {
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
    phone
}

/// Scenario: a running actor warms up to a new peer — admitted, heard, and
/// never restarted. Also pins the honest limit: admission replays nothing,
/// so the peer sends again (the delivery denied before the admission was
/// dropped, not queued).
#[test]
fn an_admitted_endpoint_is_heard_without_a_restart() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, events, _) = stage(&net, &timer, &[]);
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        // Before the admission: silence (deny-by-default), observed at home.
        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "asker"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();
        runtime.step(&name("asker")).await.unwrap();
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[Event::DeliveryDenied {
                from: app.clone(),
                to: address("bb", "asker"),
            }]
        );

        // The operator admits the endpoint on the running actor.
        runtime.admit(&name("asker"), endpoint("aa")).unwrap();

        // The peer sends again — nothing is replayed for it — and is heard.
        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "asker"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(2)).await.unwrap();
        assert!(matches!(
            runtime.step(&name("asker")).await.unwrap(),
            Stepped::Delivered
        ));
        loop {
            match process(&app_list, app_inbox.next().await.unwrap()) {
                Inbound::AckReceived(ack) => {
                    assert!(phone.note_ack(&ack));
                    break;
                }
                Inbound::Turn { turn, .. } => {
                    phone.on_incoming(&turn).unwrap();
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
    });
}

/// Scenario: revocation bites on the very next delivery and spares the
/// rest — one peer revoked mid-conversation is denied and observed; the
/// other peer's conversation is untouched.
#[test]
fn a_revocation_bites_on_the_next_delivery_and_spares_the_rest() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, events, _) = stage(&net, &timer, &["aa", "cc"]);

        let kept = address("aa", "app");
        let mut kept_inbox = net.register(kept.clone()).unwrap();
        let mut kept_list = Allowlist::empty();
        kept_list.admit(endpoint("bb"));
        let doomed = address("cc", "app");
        let mut doomed_inbox = net.register(doomed.clone()).unwrap();
        let mut doomed_list = Allowlist::empty();
        doomed_list.admit(endpoint("bb"));

        let mut kept_phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &kept,
            &kept_list,
            &mut kept_inbox,
            1,
        )
        .await;
        let mut doomed_phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &doomed,
            &doomed_list,
            &mut doomed_inbox,
            2,
        )
        .await;

        // The operator revokes cc mid-conversation.
        let failed = runtime
            .revoke(&name("asker"), &endpoint("cc"))
            .await
            .unwrap();
        assert_eq!(failed, 1);

        // cc's next turn — its answer to the party's question — is denied,
        // and the denial observed at home.
        doomed_phone
            .send_turn(Turn::Close {
                response: Response {
                    id: RequestId([200; 16]),
                    body: vec![],
                },
            })
            .await
            .unwrap();
        runtime.step(&name("asker")).await.unwrap();
        assert!(events.lock().unwrap().iter().any(|e| matches!(
            e,
            Event::DeliveryDenied { from, .. } if *from == doomed
        )));

        // aa's conversation is untouched: its closing turn is delivered
        // (a Close poses no request and earns no ack), and no denial ever
        // names aa.
        kept_phone
            .send_turn(Turn::Close {
                response: Response {
                    id: RequestId([200; 16]),
                    body: vec![],
                },
            })
            .await
            .unwrap();
        assert!(matches!(
            runtime.step(&name("asker")).await.unwrap(),
            Stepped::Delivered
        ));
        assert!(!events.lock().unwrap().iter().any(|e| matches!(
            e,
            Event::DeliveryDenied { from, .. } if *from == kept
        )));
        drop(kept_inbox);
    });
}

/// Scenario: revocation fails the live exchange at once — positive
/// evidence, the timer never moves, and the failure names the revocation.
#[test]
fn a_revocation_fails_the_live_exchange_at_once() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, events, failures) = stage(&net, &timer, &["aa"]);
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let _phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &app,
            &app_list,
            &mut app_inbox,
            1,
        )
        .await;

        // The operator revokes. The timer never moves: this is positive
        // evidence, not a deadline.
        let failed = runtime
            .revoke(&name("asker"), &endpoint("aa"))
            .await
            .unwrap();
        assert_eq!(failed, 1);
        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[(app.clone(), FailureCause::Revoked)]
        );
        assert!(events.lock().unwrap().iter().any(|e| matches!(
            e,
            Event::ExchangeFailed { peer, cause: FailureCause::Revoked, .. } if *peer == app
        )));
    });
}

/// Ander's hazard-4 pin: revoking twice fails the exchange exactly once.
/// Idempotence must hold at the observer, not only in the set — the second
/// revocation finds no live exchange and emits nothing.
#[test]
fn a_second_revocation_emits_nothing() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, events, failures) = stage(&net, &timer, &["aa"]);
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let _phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &app,
            &app_list,
            &mut app_inbox,
            1,
        )
        .await;

        assert_eq!(
            runtime
                .revoke(&name("asker"), &endpoint("aa"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            runtime
                .revoke(&name("asker"), &endpoint("aa"))
                .await
                .unwrap(),
            0
        );

        let failed_events = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, Event::ExchangeFailed { .. }))
            .count();
        assert_eq!(failed_events, 1);
        assert_eq!(failures.lock().unwrap().len(), 1);
    });
}

/// Scenario: the conversation survives the revocation — re-admitted, the
/// same peer opens a fresh exchange in the same conversation.
#[test]
fn a_readmitted_endpoint_opens_a_fresh_exchange() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, _, _) = stage(&net, &timer, &["aa"]);
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let _phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &app,
            &app_list,
            &mut app_inbox,
            1,
        )
        .await;
        runtime
            .revoke(&name("asker"), &endpoint("aa"))
            .await
            .unwrap();

        // The operator re-admits; the peer's fresh Open is served — the
        // conversation survived the failed exchange.
        runtime.admit(&name("asker"), endpoint("aa")).unwrap();
        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "asker"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(2)).await.unwrap();
        assert!(matches!(
            runtime.step(&name("asker")).await.unwrap(),
            Stepped::Delivered
        ));
        loop {
            match process(&app_list, app_inbox.next().await.unwrap()) {
                Inbound::AckReceived(ack) => {
                    assert!(phone.note_ack(&ack));
                    break;
                }
                Inbound::Turn { turn, .. } => {
                    phone.on_incoming(&turn).unwrap();
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
    });
}

/// Scenario: a key rotates without the actor missing a beat — the old key
/// is denied (and the denial observed), the new key is served, and the
/// actor never restarted. The console-key story, in miniature.
#[test]
fn a_key_rotates_without_the_actor_missing_a_beat() {
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

        // ops.operator in miniature: a harness actor admitting the old
        // console key only.
        let mut list = Allowlist::empty();
        list.admit(endpoint("0c"));
        runtime
            .install(name("operator"), list, patience(), ActorKind::Harness)
            .unwrap();
        let operator = address("bb", "operator");

        let old_console = address("0c", "console");
        let mut old_inbox = net.register(old_console.clone()).unwrap();
        let new_console = address("1c", "console");
        let mut new_inbox = net.register(new_console.clone()).unwrap();

        // The old key works today.
        net.transport()
            .send(&old_console, &operator, Message::Harness(Harness::Ping))
            .await
            .unwrap();
        runtime.step(&name("operator")).await.unwrap();
        let delivery = old_inbox.next().await.unwrap();
        assert_eq!(delivery.message, Message::Harness(Harness::Pong));

        // Rotation: admit the new key, then revoke the old — two calls on
        // the running actor, no restart.
        runtime.admit(&name("operator"), endpoint("1c")).unwrap();
        runtime
            .revoke(&name("operator"), &endpoint("0c"))
            .await
            .unwrap();

        // The old key's next command is denied, and the denial observed.
        net.transport()
            .send(&old_console, &operator, Message::Harness(Harness::Ping))
            .await
            .unwrap();
        runtime.step(&name("operator")).await.unwrap();
        assert!(events.lock().unwrap().iter().any(|e| matches!(
            e,
            Event::DeliveryDenied { from, .. } if *from == old_console
        )));

        // The new key is served.
        net.transport()
            .send(&new_console, &operator, Message::Harness(Harness::Ping))
            .await
            .unwrap();
        runtime.step(&name("operator")).await.unwrap();
        let delivery = new_inbox.next().await.unwrap();
        assert_eq!(delivery.message, Message::Harness(Harness::Pong));
    });
}

/// Scenario: revoking a stranger changes nothing — a quiet no-op, and the
/// admitted endpoint's deliveries continue unaffected.
#[test]
fn revoking_a_stranger_is_a_quiet_no_op() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, events, failures) = stage(&net, &timer, &["aa"]);
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));

        let failed = runtime
            .revoke(&name("asker"), &endpoint("dd"))
            .await
            .unwrap();
        assert_eq!(failed, 0);
        assert!(events.lock().unwrap().is_empty());
        assert!(failures.lock().unwrap().is_empty());

        // aa is still heard.
        let _phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &app,
            &app_list,
            &mut app_inbox,
            1,
        )
        .await;
    });
}

/// Scenario: revoking the last guest restores deny-by-default silence.
#[test]
fn revoking_the_last_guest_restores_deny_by_default() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, events, _) = stage(&net, &timer, &["aa"]);
        let app = address("aa", "app");
        let _app_inbox = net.register(app.clone()).unwrap();

        runtime
            .revoke(&name("asker"), &endpoint("aa"))
            .await
            .unwrap();

        // The actor now receives nothing from anyone — exactly as if it had
        // been installed with an empty list. "Anyone" is two claims: the
        // endpoint just revoked, and an endpoint never admitted at all
        // (deny-by-default's own half of R3).
        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "asker"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();
        runtime.step(&name("asker")).await.unwrap();

        let stranger = address("ee", "stranger");
        let _stranger_inbox = net.register(stranger.clone()).unwrap();
        let mut stranger_phone = Phone::converse(
            stranger.clone(),
            address("bb", "asker"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        stranger_phone.open(request(2)).await.unwrap();
        runtime.step(&name("asker")).await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[
                Event::DeliveryDenied {
                    from: app,
                    to: address("bb", "asker"),
                },
                Event::DeliveryDenied {
                    from: stranger,
                    to: address("bb", "asker"),
                }
            ]
        );
    });
}

/// The reviewer's coverage ask: one endpoint can host several actor names,
/// so a single revocation may have to fail several conversations — and the
/// public count must say so. Pinned here because every other test has at
/// most one conversation per endpoint, so a sweep silently narrowed to
/// first-match-only (the `sweep_dead` shape next door) would pass the rest
/// of the suite.
#[test]
fn one_revocation_fails_every_conversation_at_that_endpoint() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let (mut runtime, events, failures) = stage(&net, &timer, &["aa"]);

        let first = address("aa", "app1");
        let mut first_inbox = net.register(first.clone()).unwrap();
        let second = address("aa", "app2");
        let mut second_inbox = net.register(second.clone()).unwrap();
        let mut list = Allowlist::empty();
        list.admit(endpoint("bb"));

        let _first_phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &first,
            &list,
            &mut first_inbox,
            1,
        )
        .await;
        let _second_phone = open_live_exchange(
            &net,
            &timer,
            &mut runtime,
            &second,
            &list,
            &mut second_inbox,
            2,
        )
        .await;

        // One revocation; both conversations at that endpoint fail, and the
        // count says two.
        let failed = runtime
            .revoke(&name("asker"), &endpoint("aa"))
            .await
            .unwrap();
        assert_eq!(failed, 2);
        let mut told: Vec<Address> = failures
            .lock()
            .unwrap()
            .iter()
            .map(|(peer, cause)| {
                assert_eq!(*cause, FailureCause::Revoked);
                peer.clone()
            })
            .collect();
        told.sort_by_key(|a| a.name.to_string());
        assert_eq!(told, vec![first.clone(), second.clone()]);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, Event::ExchangeFailed { .. }))
                .count(),
            2
        );
    });
}

/// Ander's hazard-1 pin, the initiator's side: the runtime cannot reach an
/// app-held Phone, so the application that revokes on its own allowlist
/// relays the verdict — and the Phone fails the live exchange at once,
/// exactly as it does for a reported death.
#[test]
fn a_phone_learns_of_revocation() {
    block_on(async {
        let net = MemoryNet::new();
        let timer = TestTimer::new();
        let app = address("aa", "app");
        let _app_inbox = net.register(app.clone()).unwrap();
        let peer = address("bb", "cutoff");
        let _their_inbox = net.register(peer.clone()).unwrap();

        let mut app_list = Allowlist::empty();
        app_list.admit(endpoint("bb"));
        let mut phone = Phone::converse(
            app.clone(),
            peer,
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();

        // The application revokes bb on its own list and tells its Phone.
        // The timer never moves: positive evidence, not a deadline.
        app_list.revoke(&endpoint("bb"));
        assert_eq!(
            phone.on_revoked(&endpoint("bb")),
            Some(FailureCause::Revoked)
        );
        assert_eq!(phone.state(), TurnState::Failed);

        // A revocation of someone else's peer changes nothing, and a dead
        // exchange cannot die twice.
        assert_eq!(phone.on_revoked(&endpoint("cc")), None);
        assert_eq!(phone.on_revoked(&endpoint("bb")), None);
    });
}
