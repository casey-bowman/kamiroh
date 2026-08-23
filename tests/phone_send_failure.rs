//! A refused send fails the exchange at once (decision 26).
//!
//! History: this file began as Ander's review reproducer for the wedge —
//! a Phone whose transport send failed advanced its turn machine but armed
//! no waits, leaving the exchange permanently un-failable. Casey ruled the
//! remedy (2026-08-20): a refused send is positive local evidence, so the
//! exchange is declared dead immediately, and this test now pins the ruled
//! behavior instead of demonstrating the bug.

use std::time::Duration;

use kamiroh_adapter_memory::MemoryNet;
use kamiroh_adapter_memory::testing::{TestTimer, block_on};
use kamiroh_app::phone::{Phone, PhoneError};
use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::deadline::Deadlines;
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::hex::Hex;
use kamiroh_domain::protocol::{TurnError, TurnState};
use kamiroh_domain::vocabulary::{Request, RequestId, Response, Turn};

fn address(e: &str, n: &str) -> Address {
    Address::new(
        EndpointId::new(Hex::new(e).unwrap()),
        ActorName::new(n).unwrap(),
    )
}

fn patience() -> Deadlines {
    Deadlines::new(Duration::from_secs(5), Duration::from_secs(60))
}

#[test]
fn a_refused_send_fails_the_exchange_at_once() {
    let net = MemoryNet::new();
    let me = address("aa", "app");
    let _inbox = net.register(me.clone()).unwrap();

    // The peer is never bound, so the memory transport refuses the send.
    let peer = address("bb", "nobody");
    let timer = TestTimer::new();
    let mut phone = Phone::converse(me, peer, net.transport(), patience(), timer.clone());

    let sent = block_on(phone.open(Request {
        id: RequestId([1; 16]),
        body: vec![],
    }));
    assert!(
        matches!(sent, Err(PhoneError::Transport(_))),
        "the caller learns the send failed: {sent:?}"
    );

    // The exchange is dead on the spot — no wait to race, nothing armed.
    assert_eq!(phone.state(), TurnState::Failed);
    assert!(phone.next_due().is_none());

    // Anything but a fresh Open is refused, exactly as after a timeout.
    let late = Turn::Close {
        response: Response {
            id: RequestId([1; 16]),
            body: vec![],
        },
    };
    assert_eq!(
        phone.on_incoming(&late),
        Err(PhoneError::Turn(TurnError::ExchangeFailed))
    );

    // The conversation survives: once the peer exists, a fresh exchange
    // opens and arms its waits normally.
    let _peer_inbox = net.register(address("bb", "nobody")).unwrap();
    block_on(phone.open(Request {
        id: RequestId([2; 16]),
        body: vec![],
    }))
    .expect("a fresh exchange opens after the failure");
    assert!(matches!(phone.state(), TurnState::AwaitingTheirTurn { .. }));
    assert!(
        phone.next_due().is_some(),
        "the new exchange's waits are armed"
    );
}
