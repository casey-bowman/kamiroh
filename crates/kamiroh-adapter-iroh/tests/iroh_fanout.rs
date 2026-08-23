//! Story 4 over real sockets (`ARCHITECTURE.md`, decision 29): one QUIC
//! frame opens exchanges with several Kameo-hosted actors at one Iroh
//! endpoint; the batch receipt comes back claimed-from the mailroom; and
//! an absent sibling discloses nothing on the wire.

use std::time::Duration;

use kamiroh_adapter_iroh::IrohNet;
use kamiroh_adapter_kameo::{KameoRuntime, TokioTimer};
use kamiroh_app::inbound::{Inbound, process};
use kamiroh_app::phone::Phone;
use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::allowlist::Allowlist;
use kamiroh_domain::deadline::Deadlines;
use kamiroh_domain::protocol::{TurnProgress, TurnState};
use kamiroh_domain::secret::Secret;
use kamiroh_domain::vocabulary::{Request, RequestId};
use kamiroh_ports::{BATCH_RECEIPT_NAME, Inbox, Registry};
use tokio::time::timeout;

fn patience() -> Deadlines {
    Deadlines::new(Duration::from_secs(5), Duration::from_secs(60))
}

fn name(s: &str) -> ActorName {
    ActorName::new(s).unwrap()
}

#[tokio::test]
async fn one_frame_opens_conversations_with_two_hosted_actors() {
    // Two endpoints on loopback; relays and discovery disabled.
    let net_a = IrohNet::bind(&Secret::new(vec![3; 32])).await.unwrap();
    let net_b = IrohNet::bind(&Secret::new(vec![4; 32])).await.unwrap();
    let addr_a = net_a.addr().await.unwrap();
    let addr_b = net_b.addr().await.unwrap();
    let id_a = net_b.add_peer(addr_a);
    let id_b = net_a.add_peer(addr_b);

    // Endpoint B: a Kameo runtime hosting two echo actors admitting A —
    // deliberately NOTHING batch-aware anywhere on this side (R5: the
    // fan-in is the transport's affair; the runtime never knows).
    let runtime = KameoRuntime::new(id_b.clone(), net_b.transport(), net_b.clone(), patience());
    for w in ["w1", "w2"] {
        let mut allow = Allowlist::empty();
        allow.admit(id_a.clone());
        runtime
            .install_party(
                name(w),
                allow,
                patience(),
                Box::new(kamiroh_app::parties::EchoParty::new()),
            )
            .unwrap();
    }

    // Endpoint A: the controller.
    let controller = Address::new(id_a.clone(), name("controller"));
    let mut net_a_registry = net_a.clone();
    let mut controller_inbox = net_a_registry.bind(&controller).unwrap();
    let mut controller_list = Allowlist::empty();
    controller_list.admit(id_b.clone());

    // One frame, three names — two real, one ghost (the absent sibling
    // discloses nothing; its phone just goes silent).
    let mut phones = Phone::open_many(
        controller.clone(),
        id_b.clone(),
        vec![name("w1"), name("w2"), name("ghost")],
        Request {
            id: RequestId([9; 16]),
            body: vec![9],
        },
        net_a.transport(),
        patience(),
        TokioTimer,
    )
    .await
    .unwrap();

    // Drive the controller's inbox: the batch receipt (claimed-from the
    // mailroom) settles every phone's ack wait; then the two real workers
    // conclude their exchanges. Workers' own pairwise acks settle nothing.
    let mut receipt_seen = false;
    let mut concluded = 0;
    while concluded < 2 {
        let delivery = timeout(Duration::from_secs(10), controller_inbox.next())
            .await
            .expect("timed out")
            .expect("inbox closed");
        let from = delivery.from.clone();
        match process(&controller_list, delivery) {
            Inbound::AckReceived(ack) => {
                let settled: usize = phones
                    .iter_mut()
                    .map(|p| usize::from(p.note_ack(&ack)))
                    .sum();
                if from.name.as_str() == BATCH_RECEIPT_NAME {
                    assert_eq!(settled, 3, "the one receipt settles every wait");
                    receipt_seen = true;
                } else {
                    assert_eq!(settled, 0, "later pairwise acks settle nothing");
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
    assert!(receipt_seen, "the batch receipt arrived on the wire");
    for w in ["w1", "w2"] {
        assert_eq!(
            phones
                .iter()
                .find(|p| p.peer().name == name(w))
                .unwrap()
                .state(),
            TurnState::Idle
        );
    }
    // The ghost's phone heard nothing and still awaits a turn — silence
    // is the deadline's job, and the wire disclosed nothing about it.
    assert!(matches!(
        phones
            .iter()
            .find(|p| p.peer().name == name("ghost"))
            .unwrap()
            .state(),
        TurnState::AwaitingTheirTurn { .. }
    ));
}
