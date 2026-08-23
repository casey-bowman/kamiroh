//! Story 2 of the workshop-2 example-mapping session, pinned by test: a
//! vanished peer fails loudly (`ARCHITECTURE.md`, decision 27; scenarios in
//! `features/`). Deterministic: deaths are injected by the memory net's
//! fault levers, partitions buffer like QUIC retransmission, and time is the
//! hand-cranked `TestTimer`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kamiroh::adapter_memory::MemoryNet;
use kamiroh::adapter_memory::testing::{TestTimer, block_on};
use kamiroh::app::inbound::{Inbound, process};
use kamiroh::app::phone::Phone;
use kamiroh::app::runtime::{LocalRuntime, Stepped};
use kamiroh::app::wait::{Waited, next_or_deadline_or_death};
use kamiroh::domain::actor::{ActorName, Address};
use kamiroh::domain::allowlist::Allowlist;
use kamiroh::domain::deadline::{DeadlineKind, Deadlines, FailureCause};
use kamiroh::domain::endpoint::EndpointId;
use kamiroh::domain::hex::Hex;
use kamiroh::domain::protocol::{TurnProgress, TurnState};
use kamiroh::domain::vocabulary::{Request, RequestId, Response, Turn};
use kamiroh::ports::{DeathWatch as _, Event, Inbox as _, Party, Timer as _};

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

/// Scenario: the peer endpoint dies mid-exchange — the exchange fails at
/// once, well before any deadline; and the conversation (indeed the actor)
/// survives to serve a fresh exchange when the peer returns.
#[test]
fn a_killed_peer_fails_live_exchanges_at_once_and_the_conversation_survives() {
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
        runtime.watch_deaths(net.death_watch());
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

        // The app opens; the party answers with its own question and now
        // awaits the app's turn — a live exchange.
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

        // The app's container is killed. The timer never moves: the failure
        // below is positive evidence, not a deadline.
        net.kill_endpoint(&endpoint("aa"));
        let stepped = runtime.step(&name("asker")).await.unwrap();
        assert_eq!(
            stepped,
            Stepped::PeerVanished {
                endpoint: endpoint("aa"),
                exchanges_failed: 1,
            }
        );
        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[(app.clone(), FailureCause::Disconnected)]
        );
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[Event::ExchangeFailed {
                local: asker,
                peer: app.clone(),
                cause: FailureCause::Disconnected,
            }]
        );

        // The peer returns (a new container, same endpoint identity): its
        // fresh Open is admitted and served — the conversation survived.
        let mut app_inbox = net.register(app.clone()).unwrap();
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
        // The ack proves the delivery was processed, not tombstone-dropped.
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

/// Scenario: a wire blip is not a death — the partition holds deliveries
/// the way QUIC retransmission holds packets, and when it heals within the
/// deadline the exchange continues in the same conversation.
#[test]
fn a_wire_blip_is_not_a_death() {
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
        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        runtime
            .install_party(
                name("echo"),
                list,
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
        // The opening turn is delivered; then the wire drops.
        phone.open(request(7)).await.unwrap();
        net.partition(&endpoint("aa"), &endpoint("bb"));

        // The peer processes and answers into the blip: its ack and close
        // are held, silently — the wire is quiet, nothing errors.
        runtime.step(&name("echo")).await.unwrap();

        // The wire returns within every deadline; the held deliveries flow.
        timer.advance(Duration::from_secs(3));
        net.heal(&endpoint("aa"), &endpoint("bb"));

        let mut concluded = false;
        while !concluded {
            match process(&app_list, app_inbox.next().await.unwrap()) {
                Inbound::AckReceived(ack) => {
                    assert!(phone.note_ack(&ack));
                }
                Inbound::Turn { turn, .. } => {
                    assert_eq!(phone.on_incoming(&turn).unwrap(), TurnProgress::Concluded);
                    concluded = true;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
        assert_eq!(phone.state(), TurnState::Idle);
    });
}

/// Scenario: a silent death is caught by the backstop — a partition that
/// never heals looks like nothing at all, and the deadline is what fails
/// the exchange.
#[test]
fn a_partition_that_never_heals_is_caught_by_the_deadline_backstop() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let peer = address("bb", "frozen");
        let _their_inbox = net.register(peer.clone()).unwrap();

        let timer = TestTimer::new();
        let mut phone = Phone::converse(
            app.clone(),
            peer,
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();
        net.partition(&endpoint("aa"), &endpoint("bb"));

        // Nothing arrives — no ack, no turn, no death evidence. The ack
        // deadline (the earliest wait) is what finally speaks.
        let (kind, due) = phone.next_due().unwrap();
        assert_eq!(kind, DeadlineKind::Ack);
        timer.advance(Duration::from_secs(6));
        let waited = next_or_deadline_or_death(
            &mut app_inbox,
            Some(phone.timer().sleep_until(due)),
            net.death_watch().next_death(),
        )
        .await;
        assert!(matches!(waited, Waited::DeadlineElapsed));
        assert_eq!(
            phone.on_deadline_elapsed(),
            Some(FailureCause::DeadlineElapsed(DeadlineKind::Ack))
        );
        assert_eq!(phone.state(), TurnState::Failed);
    });
}

/// Scenario: the initiator's side of a peer death — the pump races the
/// death watch, and the Phone fails the exchange on the evidence, well
/// before any deadline.
#[test]
fn a_phone_learns_of_peer_death_from_the_watch() {
    block_on(async {
        let net = MemoryNet::new();
        let app = address("aa", "app");
        let mut app_inbox = net.register(app.clone()).unwrap();
        let peer = address("bb", "doomed");
        let _their_inbox = net.register(peer.clone()).unwrap();

        let timer = TestTimer::new();
        let mut watch = net.death_watch();
        let mut phone = Phone::converse(
            app.clone(),
            peer,
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();

        // The peer's container is killed; the timer never moves.
        net.kill_endpoint(&endpoint("bb"));
        let (_, due) = phone.next_due().unwrap();
        let waited = next_or_deadline_or_death(
            &mut app_inbox,
            Some(phone.timer().sleep_until(due)),
            watch.next_death(),
        )
        .await;
        let Waited::PeerDeath(dead) = waited else {
            panic!("expected death evidence, got {waited:?}");
        };
        assert_eq!(dead, endpoint("bb"));
        assert_eq!(phone.on_peer_death(&dead), Some(FailureCause::Disconnected));
        assert_eq!(phone.state(), TurnState::Failed);
        // A death for someone else's peer changes nothing, and a dead
        // exchange cannot die twice.
        assert_eq!(phone.on_peer_death(&endpoint("cc")), None);
        assert_eq!(phone.on_peer_death(&dead), None);
    });
}

/// External-review pin (2026-08-23): a stock party that tracks its own
/// turn machine must survive a failed exchange the way the runtime does.
/// A CountdownParty mid-exchange (it has posed a Continue) sees its peer
/// killed; when the peer returns under the same identity, its fresh Open
/// must be answered — not swallowed as MustAnswerFirst by a shadow
/// machine that never heard the exchange failed.
#[test]
fn a_countdown_party_answers_a_fresh_open_after_its_peer_died() {
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
        runtime.watch_deaths(net.death_watch());
        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        runtime
            .install_party(
                name("counter"),
                list,
                patience(),
                Box::new(kamiroh::app::parties::CountdownParty::new(2)),
            )
            .unwrap();

        // Open; the countdown answers with a question of its own — its
        // shadow machine is now mid-exchange, awaiting the app's turn.
        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "counter"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(1)).await.unwrap();
        runtime.step(&name("counter")).await.unwrap();
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

        // The app's container dies mid-exchange; the death fails the
        // exchange — and tells the party, whose shadow machine fails too.
        net.kill_endpoint(&endpoint("aa"));
        assert!(matches!(
            runtime.step(&name("counter")).await.unwrap(),
            Stepped::PeerVanished { .. }
        ));

        // The peer returns under the same identity; its fresh Open must
        // be ANSWERED — the party's reply arriving is the proof the
        // shadow machine did not swallow it.
        let mut app_inbox = net.register(app.clone()).unwrap();
        let mut phone = Phone::converse(
            app.clone(),
            address("bb", "counter"),
            net.transport(),
            patience(),
            timer.clone(),
        );
        phone.open(request(2)).await.unwrap();
        assert!(matches!(
            runtime.step(&name("counter")).await.unwrap(),
            Stepped::Delivered
        ));
        let mut answered = false;
        while !answered {
            match process(&app_list, app_inbox.next().await.unwrap()) {
                Inbound::AckReceived(ack) => {
                    phone.note_ack(&ack);
                }
                Inbound::Turn { turn, .. } => {
                    phone.on_incoming(&turn).unwrap();
                    answered = true;
                }
                other => panic!("unexpected inbound: {other:?}"),
            }
        }
    });
}
