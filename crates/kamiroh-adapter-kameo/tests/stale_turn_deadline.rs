//! REPRODUCER (Ander, review of cowork/timeouts): the turn-deadline stale
//! report is not filtered by the `DeadlineElapsed` handler's re-check.
//!
//! `still_pending` for a Turn deadline asks only `waits.turn.is_some()`.
//! That is true again as soon as a *new* turn wait is armed, so a report
//! from an already-superseded timer task kills a live exchange.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kamiroh_adapter_kameo::KameoRuntime;
use kamiroh_adapter_memory::MemoryNet;
use kamiroh_app::inbound::{Inbound, process};
use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::allowlist::Allowlist;
use kamiroh_domain::deadline::Deadlines;
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::hex::Hex;
use kamiroh_domain::vocabulary::{Message, Request, RequestId, Response, Turn};
use kamiroh_ports::{Event, Inbox, Party, Transport};

fn endpoint(s: &str) -> EndpointId {
    EndpointId::new(Hex::new(s).unwrap())
}
fn name(s: &str) -> ActorName {
    ActorName::new(s).unwrap()
}
fn address(e: &str, n: &str) -> Address {
    Address::new(endpoint(e), name(n))
}

/// Answers `aa` instantly (always continuing, so a fresh turn wait is armed
/// each round); stalls on `cc` long enough to block the host's mailbox.
struct SlowParty {
    next: u8,
}

impl Party for SlowParty {
    async fn on_turn(&mut self, from: &Address, turn: Turn) -> Option<Turn> {
        let answering = turn.request()?.id;
        if from.endpoint == endpoint("cc") {
            tokio::time::sleep(Duration::from_millis(400)).await;
            return Some(Turn::Close {
                response: Response {
                    id: answering,
                    body: vec![],
                },
            });
        }
        self.next += 1;
        Some(Turn::Continue {
            response: Response {
                id: answering,
                body: vec![],
            },
            request: Request {
                id: RequestId([self.next; 16]),
                body: vec![],
            },
        })
    }
}

/// Await the next turn addressed to a hand-rolled peer, skipping acks.
async fn next_turn(inbox: &mut kamiroh_adapter_memory::MemoryInbox, list: &Allowlist) -> Turn {
    loop {
        let delivery = tokio::time::timeout(Duration::from_secs(5), inbox.next())
            .await
            .expect("timed out")
            .expect("inbox closed");
        if let Inbound::Turn { turn, .. } = process(list, delivery) {
            return turn;
        }
    }
}

#[tokio::test]
async fn stale_turn_deadline_kills_a_live_exchange() {
    let net = MemoryNet::new();
    let mut transport = net.transport();

    let a = address("aa", "app");
    let mut a_inbox = net.register(a.clone()).unwrap();
    let mut a_list = Allowlist::empty();
    a_list.admit(endpoint("bb"));

    let c = address("cc", "other");
    let _c_inbox = net.register(c.clone()).unwrap();

    // Responder: patient about acks, impatient about turns (100ms).
    let patience = Deadlines::new(Duration::from_secs(5), Duration::from_millis(100));
    let runtime = KameoRuntime::new(endpoint("bb"), net.transport(), net.clone(), patience);

    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    runtime.set_observer(move |e: Event| sink.lock().unwrap().push(e));

    let mut list = Allowlist::empty();
    list.admit(endpoint("aa"));
    list.admit(endpoint("cc"));
    runtime
        .install_party(
            name("counter"),
            list,
            patience,
            Box::new(SlowParty { next: 100 }),
        )
        .unwrap();
    let b = address("bb", "counter");

    // Round 1: A opens; B answers and arms a 100ms turn wait (T1).
    transport
        .send(
            &a,
            &b,
            Message::Turn(Turn::Open {
                request: Request {
                    id: RequestId([1; 16]),
                    body: vec![],
                },
            }),
        )
        .await
        .unwrap();
    let first = next_turn(&mut a_inbox, &a_list).await;
    let outstanding = first.request().expect("B should pose a request").id;

    // C occupies B's mailbox for 400ms — far past T1.
    transport
        .send(
            &c,
            &b,
            Message::Turn(Turn::Open {
                request: Request {
                    id: RequestId([9; 16]),
                    body: vec![],
                },
            }),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    // A answers well inside its deadline, but B is busy: the delivery queues
    // behind C's think, and T1 fires while it waits.
    transport
        .send(
            &a,
            &b,
            Message::Turn(Turn::Continue {
                response: Response {
                    id: outstanding,
                    body: vec![],
                },
                request: Request {
                    id: RequestId([2; 16]),
                    body: vec![],
                },
            }),
        )
        .await
        .unwrap();

    // B accepts and answers A's turn — the exchange is demonstrably live.
    let second = next_turn(&mut a_inbox, &a_list).await;
    assert!(second.response().is_some(), "B answered A's turn");

    // Conclude at once, so B holds no legitimately-armed turn wait: from here
    // on, any ExchangeFailed for A can only come from the superseded T1.
    let last = second.request().expect("B poses another request").id;
    transport
        .send(
            &a,
            &b,
            Message::Turn(Turn::Close {
                response: Response {
                    id: last,
                    body: vec![],
                },
            }),
        )
        .await
        .unwrap();

    // Now let the superseded T1 report land.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let seen = events.lock().unwrap().clone();
    let killed = seen
        .iter()
        .any(|e| matches!(e, Event::ExchangeFailed { peer, .. } if *peer == a));
    assert!(
        !killed,
        "BUG: B failed A's exchange after accepting and answering A's turn. Events: {seen:#?}"
    );
}
