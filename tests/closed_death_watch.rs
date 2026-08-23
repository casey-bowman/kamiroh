//! REPRODUCER (Ander, review of cowork/disconnects): `next_or_deadline_or_death`
//! polls the death future again after it has already completed with `None`.
//!
//! The `DeathWatch` port documents `None` as "the watch is closed". Neither
//! shipped watch ever closes, so nothing in-tree hits this — but a
//! third-party watch that does will panic the pump.

use std::future::{Future, Pending};
use std::time::Duration;

use kamiroh::adapter_memory::MemoryNet;
use kamiroh::adapter_memory::testing::block_on;
use kamiroh::app::wait::{Waited, next_or_deadline_or_death};
use kamiroh::domain::actor::{ActorName, Address};
use kamiroh::domain::endpoint::EndpointId;
use kamiroh::domain::hex::Hex;
use kamiroh::domain::vocabulary::{Message, Request, RequestId};
use kamiroh::ports::Transport;

fn address(e: &str, n: &str) -> Address {
    Address::new(
        EndpointId::new(Hex::new(e).unwrap()),
        ActorName::new(n).unwrap(),
    )
}

#[test]
fn a_closed_death_watch_does_not_panic_the_race() {
    let net = MemoryNet::new();
    let me = address("aa", "app");
    let mut inbox = net.register(me.clone()).unwrap();

    // A watch that is already closed: `None`, exactly as the port documents.
    let death: std::pin::Pin<Box<dyn Future<Output = Option<EndpointId>> + Send>> =
        Box::pin(async { None });

    // Nothing is queued yet, so the race must park. A delivery arrives after
    // it has parked, waking it for a second poll.
    let sender = net.clone();
    let peer = address("bb", "peer");
    let target = me.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        let mut t = sender.transport();
        let _ = block_on(t.send(
            &peer,
            &target,
            Message::Request(Request {
                id: RequestId([1; 16]),
                body: vec![],
            }),
        ));
    });

    let waited = block_on(next_or_deadline_or_death(
        &mut inbox,
        None::<Pending<()>>,
        death,
    ));
    assert!(
        matches!(waited, Waited::Delivery(Some(_))),
        "a closed watch should drop out of the race, not panic it: {waited:?}"
    );
}
