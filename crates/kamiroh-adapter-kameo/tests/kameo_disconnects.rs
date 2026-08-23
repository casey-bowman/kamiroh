//! Story 2 in the Kameo runtime (`ARCHITECTURE.md`, decision 27): the death
//! fan-out — transport evidence reaching every hosted actor, failing live
//! exchanges at once, with the party told and the observer seeing it.

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

#[tokio::test]
async fn a_killed_peer_fails_the_hosted_partys_live_exchange_at_once() {
    let net = MemoryNet::new();
    let app = address("aa", "app");
    let mut app_inbox = net.register(app.clone()).unwrap();
    let mut app_list = Allowlist::empty();
    app_list.admit(endpoint("bb"));

    let runtime = KameoRuntime::new(endpoint("bb"), net.transport(), net.clone(), patience());
    let _death_pump = runtime.watch_deaths(net.death_watch());
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

    // The app's endpoint dies; the fan-out fails the party's exchange —
    // positive evidence, well before the 5s/60s deadlines.
    net.kill_endpoint(&endpoint("aa"));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        {
            let events = events.lock().unwrap();
            if events.iter().any(|e| {
                matches!(
                    e,
                    Event::ExchangeFailed {
                        cause: FailureCause::Disconnected,
                        ..
                    }
                )
            }) {
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "death evidence never reached the observer"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        failures.lock().unwrap().as_slice(),
        &[(app, FailureCause::Disconnected)]
    );
}
