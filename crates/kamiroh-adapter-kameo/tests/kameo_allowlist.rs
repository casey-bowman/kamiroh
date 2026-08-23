//! Story 3 in the Kameo runtime (`ARCHITECTURE.md`, decision 28): live
//! allowlist mutation through the ask-based methods — the admission takes
//! effect without a restart, a revocation fails the hosted party's live
//! exchange at once, and a second revocation emits nothing (idempotence at
//! the observer, not just in the set).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kamiroh_adapter_kameo::{KameoRuntime, TokioTimer};
use kamiroh_adapter_memory::MemoryNet;
use kamiroh_app::inbound::{Inbound, process};
use kamiroh_app::phone::Phone;
use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::allowlist::Allowlist;
use kamiroh_domain::deadline::{Deadlines, FailureCause};
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::hex::Hex;
use kamiroh_domain::vocabulary::{Request, RequestId, Response, Turn};
use kamiroh_ports::{Event, Inbox, Party};
use tokio::time::timeout;

fn endpoint(s: &str) -> EndpointId {
    EndpointId::new(Hex::new(s).unwrap())
}

fn name(s: &str) -> ActorName {
    ActorName::new(s).unwrap()
}

fn address(e: &str, n: &str) -> Address {
    Address::new(endpoint(e), name(n))
}

fn patience() -> Deadlines {
    Deadlines::new(Duration::from_secs(5), Duration::from_secs(60))
}

/// Answers an `Open` with a question of its own; records failures.
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

/// Revocation on a hosted actor: the live exchange fails at once with
/// `Revoked` through both surfaces; a second revocation is a no-op at the
/// observer (the hazard-4 pin under the real mailbox); and after a
/// re-admission the same peer's fresh Open is served — the conversation
/// survived. The `ask`-based methods make every step a real
/// synchronization point, so the test is deterministic.
#[tokio::test]
async fn a_hosted_actors_guest_list_changes_while_it_runs() {
    let net = MemoryNet::new();
    let app = address("aa", "app");
    let mut app_inbox = net.register(app.clone()).unwrap();
    let mut app_list = Allowlist::empty();
    app_list.admit(endpoint("bb"));

    let runtime = KameoRuntime::new(endpoint("bb"), net.transport(), net.clone(), patience());
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

    // Open; the party answers with its own question — a live exchange.
    let mut phone = Phone::converse(
        app.clone(),
        address("bb", "asker"),
        net.transport(),
        patience(),
        TokioTimer,
    );
    phone
        .open(Request {
            id: RequestId([1; 16]),
            body: vec![1],
        })
        .await
        .unwrap();
    loop {
        let delivery = timeout(Duration::from_secs(5), app_inbox.next())
            .await
            .expect("timed out")
            .expect("inbox closed");
        match process(&app_list, delivery) {
            Inbound::AckReceived(_) => continue,
            Inbound::Turn { turn, .. } => {
                phone.on_incoming(&turn).unwrap();
                break;
            }
            other => panic!("unexpected inbound: {other:?}"),
        }
    }

    // The operator revokes aa. Awaiting the ask means the revocation is in
    // force — and the live exchange has already failed — when it returns.
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

    // Revoking again fails nothing and emits nothing: idempotent at the
    // observer, not only in the set.
    let failed = runtime
        .revoke(&name("asker"), &endpoint("aa"))
        .await
        .unwrap();
    assert_eq!(failed, 0);
    assert_eq!(failures.lock().unwrap().len(), 1);
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, Event::ExchangeFailed { .. }))
            .count(),
        1
    );

    // The revoked peer's next turn is denied, and the denial observed.
    phone
        .send_turn(Turn::Close {
            response: Response {
                id: RequestId([200; 16]),
                body: vec![],
            },
        })
        .await
        .unwrap();
    // The denial is emitted when the host processes the delivery; poll the
    // observer, bounded so a regression fails rather than hangs.
    let denied = |events: &Arc<Mutex<Vec<Event>>>| {
        events.lock().unwrap().iter().any(|e| {
            matches!(
                e,
                Event::DeliveryDenied { from, .. } if *from == app
            )
        })
    };
    for _ in 0..500 {
        if denied(&events) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(denied(&events), "the revoked peer's turn was never denied");

    // Re-admitted, the same peer opens a fresh exchange in the same
    // conversation — served, not tombstoned.
    runtime.admit(&name("asker"), endpoint("aa")).await.unwrap();
    let mut phone = Phone::converse(
        app.clone(),
        address("bb", "asker"),
        net.transport(),
        patience(),
        TokioTimer,
    );
    phone
        .open(Request {
            id: RequestId([2; 16]),
            body: vec![2],
        })
        .await
        .unwrap();
    loop {
        let delivery = timeout(Duration::from_secs(5), app_inbox.next())
            .await
            .expect("timed out")
            .expect("inbox closed");
        match process(&app_list, delivery) {
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
}
