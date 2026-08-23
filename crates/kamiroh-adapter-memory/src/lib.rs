//! In-process transport adapter.
//!
//! Implements the [`Transport`] and [`Inbox`] ports over in-memory mailboxes,
//! so the application layer can be exercised in tests with no network
//! involved. Zero dependencies beyond the core: waiting is implemented with
//! std wakers, so any executor can drive it — including the minimal
//! [`testing::block_on`] this crate ships for tests.
//!
//! ## Trust caveat — test affordance
//!
//! [`MemoryTransport::send`] accepts the sender's `from` address as given:
//! callers can claim any origin, which is exactly what makes allowlist-denial
//! tests easy to write. Real transports must do the opposite — the receiving
//! adapter derives `Delivery::from.endpoint` from the *authenticated
//! connection*, never from the sender's claim. `kamiroh-adapter-iroh` will get
//! this from Iroh's connection handshake.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::vocabulary::{Ack, Message, Request, Turn};
use kamiroh_ports::{DeathWatch, Delivery, Inbox, Registry, Transport, batch_receipt_sender};

pub mod testing;

#[derive(Debug, Default)]
struct Mailbox {
    queue: VecDeque<Delivery>,
    waker: Option<Waker>,
}

#[derive(Debug, Default)]
struct Shared {
    mailboxes: HashMap<Address, Mailbox>,
    /// Partitioned endpoint pairs (stored in both orientations). Deliveries
    /// between them are buffered, not dropped — modeling what QUIC's
    /// retransmission does across a wire blip (decision 27).
    partitions: HashSet<(EndpointId, EndpointId)>,
    /// Deliveries held back by partitions, in send order, flushed on heal.
    held: Vec<Delivery>,
    /// Death-event queues, one per outstanding [`MemoryDeathWatch`].
    death_watchers: Vec<Arc<Mutex<DeathQueue>>>,
}

#[derive(Debug, Default)]
struct DeathQueue {
    queue: VecDeque<EndpointId>,
    waker: Option<Waker>,
}

/// An in-process "network": a registry of actor mailboxes.
///
/// Clone handles freely; they all point at the same network.
#[derive(Clone, Default)]
pub struct MemoryNet {
    shared: Arc<Mutex<Shared>>,
}

impl MemoryNet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an actor at `address`, returning the [`Inbox`] its messages
    /// arrive on. The mailbox lives until the returned inbox is dropped.
    pub fn register(&self, address: Address) -> Result<MemoryInbox, RegisterError> {
        let mut shared = self.shared.lock().expect("memory net poisoned");
        if shared.mailboxes.contains_key(&address) {
            return Err(RegisterError::AddressInUse);
        }
        shared.mailboxes.insert(address.clone(), Mailbox::default());
        Ok(MemoryInbox {
            address,
            shared: Arc::clone(&self.shared),
        })
    }

    /// Kill an endpoint, as a container-killing test harness would
    /// (decision 27; part of this adapter's contract — the memory net is a
    /// small honest simulator of a network, faults included). All the
    /// endpoint's mailboxes vanish (pending receivers see their inboxes
    /// close; senders get [`MemoryTransportError::UnknownAddress`] — the
    /// positive evidence of a failed send), and every
    /// [`MemoryDeathWatch`] reports the endpoint dead.
    pub fn kill_endpoint(&self, endpoint: &EndpointId) {
        let (wakers, watchers) = {
            let mut shared = self.shared.lock().expect("memory net poisoned");
            let doomed: Vec<Address> = shared
                .mailboxes
                .keys()
                .filter(|a| a.endpoint == *endpoint)
                .cloned()
                .collect();
            let mut wakers = Vec::new();
            for address in doomed {
                if let Some(mut mailbox) = shared.mailboxes.remove(&address)
                    && let Some(waker) = mailbox.waker.take()
                {
                    wakers.push(waker);
                }
            }
            let watchers: Vec<_> = shared.death_watchers.clone();
            (wakers, watchers)
        };
        for waker in wakers {
            waker.wake();
        }
        for watcher in watchers {
            let waker = {
                let mut q = watcher.lock().expect("death queue poisoned");
                q.queue.push_back(endpoint.clone());
                q.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    /// Partition two endpoints, as a flaky wire would: deliveries between
    /// them are silently *held* (the sender sees success — packets in
    /// flight are not errors) until [`MemoryNet::heal`] flushes them in
    /// order. No death is reported: a partition is silence, and silence is
    /// the deadline backstop's job, not positive evidence (decision 27).
    pub fn partition(&self, a: &EndpointId, b: &EndpointId) {
        let mut shared = self.shared.lock().expect("memory net poisoned");
        shared.partitions.insert((a.clone(), b.clone()));
        shared.partitions.insert((b.clone(), a.clone()));
    }

    /// Heal a partition: held deliveries flow to their mailboxes in the
    /// order they were sent, exactly as retransmission would deliver them.
    pub fn heal(&self, a: &EndpointId, b: &EndpointId) {
        let wakers = {
            let mut shared = self.shared.lock().expect("memory net poisoned");
            shared.partitions.remove(&(a.clone(), b.clone()));
            shared.partitions.remove(&(b.clone(), a.clone()));
            let (flush, keep): (Vec<Delivery>, Vec<Delivery>) =
                shared.held.drain(..).partition(|d| {
                    (d.from.endpoint == *a && d.to.endpoint == *b)
                        || (d.from.endpoint == *b && d.to.endpoint == *a)
                });
            shared.held = keep;
            let mut wakers = Vec::new();
            for delivery in flush {
                // A mailbox may have vanished meanwhile (killed): the held
                // delivery is then lost, exactly as the real wire loses
                // packets to a dead host.
                if let Some(mailbox) = shared.mailboxes.get_mut(&delivery.to) {
                    mailbox.queue.push_back(delivery);
                    if let Some(waker) = mailbox.waker.take() {
                        wakers.push(waker);
                    }
                }
            }
            wakers
        };
        for waker in wakers {
            waker.wake();
        }
    }

    /// A [`DeathWatch`] onto this network: reports endpoints killed by
    /// [`MemoryNet::kill_endpoint`].
    pub fn death_watch(&self) -> MemoryDeathWatch {
        let queue = Arc::new(Mutex::new(DeathQueue::default()));
        self.shared
            .lock()
            .expect("memory net poisoned")
            .death_watchers
            .push(Arc::clone(&queue));
        MemoryDeathWatch { queue }
    }

    /// A [`Transport`] handle onto this network.
    pub fn transport(&self) -> MemoryTransport {
        MemoryTransport {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Registry for MemoryNet {
    type Inbox = MemoryInbox;
    type Error = RegisterError;

    fn bind(&mut self, address: &Address) -> Result<Self::Inbox, Self::Error> {
        self.register(address.clone())
    }
}

/// [`Transport`] implementation over [`MemoryNet`].
#[derive(Clone)]
pub struct MemoryTransport {
    shared: Arc<Mutex<Shared>>,
}

impl Transport for MemoryTransport {
    type Error = MemoryTransportError;

    async fn send(
        &mut self,
        from: &Address,
        to: &Address,
        message: Message,
    ) -> Result<(), Self::Error> {
        let mut shared = self.shared.lock().expect("memory net poisoned");
        // A partition holds the delivery silently: the sender sees success,
        // the wire is quiet, and heal() delivers in order (decision 27).
        if shared
            .partitions
            .contains(&(from.endpoint.clone(), to.endpoint.clone()))
        {
            shared.held.push(Delivery {
                from: from.clone(),
                to: to.clone(),
                message,
            });
            return Ok(());
        }
        let mailbox = shared
            .mailboxes
            .get_mut(to)
            .ok_or(MemoryTransportError::UnknownAddress)?;
        mailbox.queue.push_back(Delivery {
            from: from.clone(),
            to: to.clone(),
            message,
        });
        if let Some(waker) = mailbox.waker.take() {
            waker.wake();
        }
        Ok(())
    }

    async fn open_many(
        &mut self,
        from: &Address,
        to_endpoint: &EndpointId,
        to_names: &[ActorName],
        request: Request,
    ) -> Result<(), Self::Error> {
        let wakers = {
            let mut shared = self.shared.lock().expect("memory net poisoned");
            // The receipt promises "the batch reached the endpoint" — so
            // there must BE an endpoint. On the wire that is physical
            // (iroh cannot mint a receipt without a successful connect);
            // here the closest honest analogue is: at least one mailbox
            // lives at the target endpoint. A batch to an endpoint with
            // no mailboxes at all errors, exactly as the pairwise send
            // does for a no-route address — while a LIVE endpoint whose
            // named actors happen not to exist still acks, because the
            // names were never part of the receipt's promise.
            // (External-review finding, 2026-08-23: without this check
            // the simulator was more generous than the wire on precisely
            // decision 29's new promise.)
            if !shared.mailboxes.keys().any(|a| a.endpoint == *to_endpoint) {
                return Err(MemoryTransportError::UnknownAddress);
            }
            let receipt = Delivery {
                from: batch_receipt_sender(to_endpoint),
                to: from.clone(),
                message: Message::Ack(Ack { id: request.id }),
            };
            let fanned: Vec<Delivery> = to_names
                .iter()
                .map(|name| Delivery {
                    from: from.clone(),
                    to: Address::new(to_endpoint.clone(), name.clone()),
                    message: Message::Turn(Turn::Open {
                        request: request.clone(),
                    }),
                })
                .collect();
            // A partition holds the whole batch — receipt included —
            // silently, exactly as it holds a pairwise send: the wire is
            // quiet, and heal() delivers in order (decision 27).
            if shared
                .partitions
                .contains(&(from.endpoint.clone(), to_endpoint.clone()))
            {
                shared.held.push(receipt);
                shared.held.extend(fanned);
                return Ok(());
            }
            let mut wakers = Vec::new();
            // The receipt first: it is the endpoint's arrival
            // acknowledgment, owed before any name is consulted — and owed
            // even when every name is unbound (decision 29: the names were
            // never part of the receipt's promise).
            if let Some(mailbox) = shared.mailboxes.get_mut(from) {
                mailbox.queue.push_back(receipt);
                if let Some(waker) = mailbox.waker.take() {
                    wakers.push(waker);
                }
            }
            for delivery in fanned {
                // Unbound names drop silently: an unbound name discloses
                // nothing (decision 29 — a per-name error would be a
                // roster oracle). Note the deliberate contrast with the
                // pairwise send above, which errors as a simulator
                // convenience.
                if let Some(mailbox) = shared.mailboxes.get_mut(&delivery.to) {
                    mailbox.queue.push_back(delivery);
                    if let Some(waker) = mailbox.waker.take() {
                        wakers.push(waker);
                    }
                }
            }
            wakers
        };
        for waker in wakers {
            waker.wake();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTransportError {
    /// No actor is registered at the target address.
    UnknownAddress,
}

impl fmt::Display for MemoryTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryTransportError::UnknownAddress => {
                f.write_str("no actor is registered at the target address")
            }
        }
    }
}

impl std::error::Error for MemoryTransportError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    /// An actor with this address is already registered.
    AddressInUse,
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterError::AddressInUse => {
                f.write_str("an actor with this address is already registered")
            }
        }
    }
}

impl std::error::Error for RegisterError {}

/// [`Inbox`] implementation over [`MemoryNet`]. Dropping it unregisters the
/// actor; subsequent sends to its address fail with
/// [`MemoryTransportError::UnknownAddress`].
#[derive(Debug)]
pub struct MemoryInbox {
    address: Address,
    shared: Arc<Mutex<Shared>>,
}

impl Inbox for MemoryInbox {
    async fn next(&mut self) -> Option<Delivery> {
        NextDelivery { inbox: self }.await
    }
}

impl Drop for MemoryInbox {
    fn drop(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.mailboxes.remove(&self.address);
        }
    }
}

struct NextDelivery<'a> {
    inbox: &'a MemoryInbox,
}

impl Future for NextDelivery<'_> {
    type Output = Option<Delivery>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared = self.inbox.shared.lock().expect("memory net poisoned");
        let Some(mailbox) = shared.mailboxes.get_mut(&self.inbox.address) else {
            return Poll::Ready(None);
        };
        match mailbox.queue.pop_front() {
            Some(delivery) => Poll::Ready(Some(delivery)),
            None => {
                mailbox.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// [`DeathWatch`] over [`MemoryNet`]: yields endpoints killed by
/// [`MemoryNet::kill_endpoint`], in order. Never closes.
pub struct MemoryDeathWatch {
    queue: Arc<Mutex<DeathQueue>>,
}

impl DeathWatch for MemoryDeathWatch {
    async fn next_death(&mut self) -> Option<EndpointId> {
        NextDeath { queue: &self.queue }.await
    }
}

struct NextDeath<'a> {
    queue: &'a Arc<Mutex<DeathQueue>>,
}

impl Future for NextDeath<'_> {
    type Output = Option<EndpointId>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut q = self.queue.lock().expect("death queue poisoned");
        match q.queue.pop_front() {
            Some(endpoint) => Poll::Ready(Some(endpoint)),
            None => {
                q.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kamiroh_domain::actor::ActorName;
    use kamiroh_domain::endpoint::EndpointId;
    use kamiroh_domain::hex::Hex;
    use kamiroh_domain::vocabulary::{Harness, Message};

    use crate::testing::block_on;

    fn address(endpoint: &str, name: &str) -> Address {
        Address::new(
            EndpointId::new(Hex::new(endpoint).unwrap()),
            ActorName::new(name).unwrap(),
        )
    }

    #[test]
    fn send_to_unknown_address_errors() {
        let net = MemoryNet::new();
        let mut t = net.transport();
        let err = block_on(t.send(
            &address("aa", "alice"),
            &address("bb", "bob"),
            Message::Harness(Harness::Ping),
        ));
        assert_eq!(err, Err(MemoryTransportError::UnknownAddress));
    }

    #[test]
    fn deliveries_arrive_in_order() {
        let net = MemoryNet::new();
        let alice = address("aa", "alice");
        let bob = address("bb", "bob");
        let mut inbox = net.register(bob.clone()).unwrap();
        let mut t = net.transport();
        block_on(async {
            t.send(&alice, &bob, Message::Harness(Harness::Ping))
                .await
                .unwrap();
            t.send(&alice, &bob, Message::Harness(Harness::Pong))
                .await
                .unwrap();
            let first = inbox.next().await.unwrap();
            let second = inbox.next().await.unwrap();
            assert_eq!(first.message, Message::Harness(Harness::Ping));
            assert_eq!(second.message, Message::Harness(Harness::Pong));
            assert_eq!(first.from, alice);
            assert_eq!(first.to, bob);
        });
    }

    #[test]
    fn duplicate_registration_is_refused() {
        let net = MemoryNet::new();
        let bob = address("bb", "bob");
        let _inbox = net.register(bob.clone()).unwrap();
        assert_eq!(net.register(bob).unwrap_err(), RegisterError::AddressInUse);
    }

    #[test]
    fn dropping_inbox_unregisters() {
        let net = MemoryNet::new();
        let bob = address("bb", "bob");
        let inbox = net.register(bob.clone()).unwrap();
        drop(inbox);
        let mut t = net.transport();
        let err = block_on(t.send(
            &address("aa", "alice"),
            &bob,
            Message::Harness(Harness::Ping),
        ));
        assert_eq!(err, Err(MemoryTransportError::UnknownAddress));
    }

    #[test]
    fn pending_receiver_is_woken_by_a_send() {
        let net = MemoryNet::new();
        let alice = address("aa", "alice");
        let bob = address("bb", "bob");
        let mut inbox = net.register(bob.clone()).unwrap();
        let net2 = net.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let mut t = net2.transport();
            block_on(t.send(&alice, &bob, Message::Harness(Harness::Ping))).unwrap();
        });
        let delivery = block_on(inbox.next()).unwrap();
        assert_eq!(delivery.message, Message::Harness(Harness::Ping));
        sender.join().unwrap();
    }
}
