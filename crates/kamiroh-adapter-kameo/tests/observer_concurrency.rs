//! Does the new `emit` lose events when two hosts emit at once?
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kamiroh_adapter_kameo::KameoRuntime;
use kamiroh_adapter_memory::MemoryNet;
use kamiroh_app::runtime::ActorKind;
use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::allowlist::Allowlist;
use kamiroh_domain::deadline::Deadlines;
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::hex::Hex;
use kamiroh_domain::vocabulary::{Message, Request, RequestId};
use kamiroh_ports::{Event, Transport};

fn endpoint(s: &str) -> EndpointId {
    EndpointId::new(Hex::new(s).unwrap())
}
fn name(s: &str) -> ActorName {
    ActorName::new(s).unwrap()
}
fn address(e: &str, n: &str) -> Address {
    Address::new(endpoint(e), name(n))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_emits_are_not_lost() {
    let net = MemoryNet::new();
    let mut transport = net.transport();
    let patience = Deadlines::new(Duration::from_secs(5), Duration::from_secs(60));
    let runtime = KameoRuntime::new(endpoint("bb"), net.transport(), net.clone(), patience);

    let seen: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    // A deliberately slow observer, to widen the window the take() opens.
    runtime.set_observer(move |e: Event| {
        std::thread::sleep(Duration::from_millis(50));
        sink.lock().unwrap().push(e);
    });

    // Two actors, each admitting nobody: every delivery is denied.
    for n in ["one", "two"] {
        runtime
            .install(name(n), Allowlist::empty(), patience, ActorKind::Plain)
            .unwrap();
    }

    // Two denials, raised as close to simultaneously as we can manage.
    let sender = address("cc", "stranger");
    for n in ["one", "two"] {
        transport
            .send(
                &sender,
                &address("bb", n),
                Message::Request(Request {
                    id: RequestId([1; 16]),
                    body: vec![],
                }),
            )
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    let got = seen.lock().unwrap().len();
    assert_eq!(got, 2, "both denials should be observable; saw {got}");
}
