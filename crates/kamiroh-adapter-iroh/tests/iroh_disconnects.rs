//! Story 2 over real sockets (`ARCHITECTURE.md`, decision 27; the Q5 ruling
//! of the mapping session): the glossary's "a conversation spans
//! connections" promise pinned by test, and death evidence flowing from
//! `Connection::closed()` to a [`DeathWatch`].

use std::time::Duration;

use kamiroh_adapter_iroh::{IrohInbox, IrohNet};
use kamiroh_adapter_kameo::{KameoRuntime, TokioTimer};
use kamiroh_app::inbound::{Inbound, process};
use kamiroh_app::parties::EchoParty;
use kamiroh_app::phone::Phone;
use kamiroh_app::runtime::ActorKind;
use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::allowlist::Allowlist;
use kamiroh_domain::deadline::Deadlines;
use kamiroh_domain::protocol::TurnProgress;
use kamiroh_domain::secret::Secret;
use kamiroh_domain::vocabulary::{Request, RequestId};
use kamiroh_ports::{DeathWatch, Inbox, Registry};
use tokio::time::timeout;

fn patience() -> Deadlines {
    Deadlines::new(Duration::from_secs(15), Duration::from_secs(60))
}

fn name(s: &str) -> ActorName {
    ActorName::new(s).unwrap()
}

fn request(n: u8) -> Request {
    Request {
        id: RequestId([n; 16]),
        body: vec![n],
    }
}

async fn next_inbound(inbox: &mut IrohInbox, allowlist: &Allowlist) -> Inbound {
    let delivery = timeout(Duration::from_secs(10), inbox.next())
        .await
        .expect("timed out waiting for a delivery")
        .expect("inbox closed");
    process(allowlist, delivery)
}

/// One complete echo exchange over `phone`, asserting ack-then-close.
async fn echo_exchange(
    phone: &mut Phone<kamiroh_adapter_iroh::IrohTransport, TokioTimer>,
    inbox: &mut IrohInbox,
    allowlist: &Allowlist,
    n: u8,
) {
    phone.open(request(n)).await.unwrap();
    loop {
        match next_inbound(inbox, allowlist).await {
            Inbound::AckReceived(_) => continue,
            Inbound::Turn { turn, .. } => {
                assert_eq!(phone.on_incoming(&turn).unwrap(), TurnProgress::Concluded);
                return;
            }
            other => panic!("unexpected inbound: {other:?}"),
        }
    }
}

/// The Q5 pin: a conversation spans connections. An exchange completes; the
/// initiator deliberately closes the underlying connection; the next
/// exchange on the *same phone* — the same conversation — re-dials and
/// completes, and the receiving side routes the fresh inbound connection to
/// the same actor.
#[tokio::test]
async fn a_conversation_spans_connections() {
    let net_a = IrohNet::bind(&Secret::new(vec![3; 32])).await.unwrap();
    let net_b = IrohNet::bind(&Secret::new(vec![4; 32])).await.unwrap();
    let addr_a = net_a.addr().await.unwrap();
    let addr_b = net_b.addr().await.unwrap();
    let id_a = net_b.add_peer(addr_a);
    let id_b = net_a.add_peer(addr_b);

    let runtime = KameoRuntime::new(id_b.clone(), net_b.transport(), net_b.clone(), patience());
    let mut echo_list = Allowlist::empty();
    echo_list.admit(id_a.clone());
    runtime
        .install_party(
            name("echo"),
            echo_list,
            patience(),
            Box::new(EchoParty::new()),
        )
        .unwrap();

    let app = Address::new(id_a.clone(), name("app"));
    let mut net_a_registry = net_a.clone();
    let mut app_inbox = net_a_registry.bind(&app).unwrap();
    let mut app_list = Allowlist::empty();
    app_list.admit(id_b.clone());

    let echo = Address::new(id_b.clone(), name("echo"));
    let mut phone = Phone::converse(app, echo, net_a.transport(), patience(), TokioTimer);

    // Exchange 1 establishes (and caches) the connection.
    echo_exchange(&mut phone, &mut app_inbox, &app_list, 1).await;

    // The wire goes away — deliberately, from this side.
    assert!(net_a.close_connection(&id_b).await);

    // Exchange 2, same phone, same conversation: the adapter re-dials and
    // the receiving side routes the new connection to the same actor.
    echo_exchange(&mut phone, &mut app_inbox, &app_list, 2).await;
}

/// Death evidence over real sockets: a peer that shuts down gracefully is
/// reported dead by the watch, within seconds — QUIC's goodbye, not a
/// deadline.
#[tokio::test]
async fn a_shutdown_peer_is_reported_dead() {
    let net_a = IrohNet::bind(&Secret::new(vec![5; 32])).await.unwrap();
    let net_b = IrohNet::bind(&Secret::new(vec![6; 32])).await.unwrap();
    let addr_a = net_a.addr().await.unwrap();
    let addr_b = net_b.addr().await.unwrap();
    let id_a = net_b.add_peer(addr_a);
    let id_b = net_a.add_peer(addr_b);

    let runtime = KameoRuntime::new(id_b.clone(), net_b.transport(), net_b.clone(), patience());
    let mut list = Allowlist::empty();
    list.admit(id_a.clone());
    runtime
        .install(name("target"), list, patience(), ActorKind::Plain)
        .unwrap();

    let app = Address::new(id_a.clone(), name("app"));
    let mut net_a_registry = net_a.clone();
    let mut app_inbox = net_a_registry.bind(&app).unwrap();
    let mut app_list = Allowlist::empty();
    app_list.admit(id_b.clone());

    // One request-ack so a connection exists to observe.
    let mut transport = net_a.transport();
    use kamiroh_domain::vocabulary::Message;
    use kamiroh_ports::Transport;
    transport
        .send(
            &app,
            &Address::new(id_b.clone(), name("target")),
            Message::Request(request(9)),
        )
        .await
        .unwrap();
    let Inbound::AckReceived(_) = next_inbound(&mut app_inbox, &app_list).await else {
        panic!("expected the ack");
    };

    // The peer says goodbye; the watch hears it, fast.
    let mut watch = net_a.death_watch();
    net_b.shutdown().await;
    let dead = timeout(Duration::from_secs(10), watch.next_death())
        .await
        .expect("death should be reported within seconds")
        .expect("watch closed");
    assert_eq!(dead, id_b);
}
