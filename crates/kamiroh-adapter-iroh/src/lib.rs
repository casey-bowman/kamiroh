//! Iroh transport adapter.
//!
//! Implements the [`Transport`] and [`Registry`] ports on real Iroh
//! connections: conversations travel as QUIC streams between endpoints
//! identified by public keys. This is the adapter kamiroh is named for.
//!
//! Design (`ARCHITECTURE.md`, decision 19):
//! - **Origin is proven, names are claimed.** The receiving side constructs
//!   `Delivery::from.endpoint` from the connection's authenticated remote
//!   key — never from frame content. Only the *name* halves ride in the
//!   frame. Forging an origin endpoint is therefore impossible; forging a
//!   name means only what the trust model already says it means.
//! - **Static peer book.** Discovery is deferred; peers are introduced
//!   explicitly via [`IrohNet::add_peer`] (matching "static configuration
//!   for the spike").
//! - **One frame per uni-stream** over a cached per-peer connection, one
//!   reconnect retry on stale connections. ALPN `kamiroh/1`. Wire format:
//!   length-implicit postcard (the stream is the frame boundary).
//! - Allowlist enforcement stays where it lives: the app layer's per-delivery
//!   admission. The adapter delivers to bound names and does nothing more.
//!
//! ## A historical note on the "assumption point" comments
//!
//! This adapter was first drafted in spike 1 against `iroh = "0.35"`-era
//! APIs **without compiling** (the cloud sandbox could not reach
//! crates.io then, and blind-writing with assumption lists was the
//! workflow — since retired, see `docs/WORKFLOW.md`). It has long since
//! been compiled, tested, reviewed, and extended against the real
//! dependency (`iroh 1.0` at spike 2's close, on ALPN `kamiroh/1`). The
//! numbered "assumption point" markers below survive as archaeology:
//! they record which guesses the first local build pass had to correct,
//! and are no longer live uncertainty.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::task::Waker;

// iroh 1.0 renamed NodeId -> EndpointId and NodeAddr -> EndpointAddr. Alias
// them back to the adapter's names, which stay distinct from the domain's own
// `EndpointId` and keep the routing/framing code below unchanged.
use iroh::endpoint::ConnectionError;
use iroh::endpoint::presets;
use iroh::{
    Endpoint, EndpointAddr as NodeAddr, EndpointId as NodeId, RelayMode, SecretKey, Watcher,
};
use tokio::sync::mpsc;

use kamiroh_domain::actor::{ActorName, Address};
use kamiroh_domain::endpoint::EndpointId;
use kamiroh_domain::hex::Hex;
use kamiroh_domain::secret::Secret;
use kamiroh_domain::vocabulary::{Ack, Message, Request, Turn};
use kamiroh_ports::{DeathWatch, Delivery, Inbox, Registry, Transport, batch_receipt_sender};

/// The kamiroh ALPN: one protocol version on the wire. Bumped to `/1`
/// when decision 29 changed the frame layout (struct -> enum): an
/// incompatible wire change bumps this number, so mismatched builds
/// refuse each other loudly at the handshake instead of silently
/// dropping frames until deadlines elapse.
pub const ALPN: &[u8] = b"kamiroh/1";

/// Cap on a single frame; a spike guard, not a protocol constant.
const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// What crosses the wire. The origin *endpoint* deliberately does not:
/// the receiver takes it from the connection's authenticated remote key.
#[derive(serde::Serialize, serde::Deserialize)]
enum Frame {
    /// One message to one actor — the pairwise send.
    Single {
        from_name: ActorName,
        to_name: ActorName,
        message: Message,
    },
    /// One opening request to several actors at this endpoint (decision
    /// 29): the receiver acks the batch once at arrival, then fans in to
    /// each bound name — unbound names disclose nothing.
    OpenMany {
        from_name: ActorName,
        to_names: Vec<ActorName>,
        request: Request,
    },
}

#[derive(Default)]
struct Router {
    /// Bound local actors: name → sender into that actor's inbox.
    bound: HashMap<ActorName, mpsc::UnboundedSender<Delivery>>,
}

struct Shared {
    endpoint: Endpoint,
    endpoint_id: EndpointId,
    router: Mutex<Router>,
    /// Static peer book: domain endpoint id → dialable iroh address.
    peers: Mutex<HashMap<EndpointId, NodeAddr>>,
    /// Cached connections per peer.
    connections: tokio::sync::Mutex<HashMap<EndpointId, iroh::endpoint::Connection>>,
    /// Death-event queues, one per outstanding [`IrohDeathWatch`]
    /// (decision 27).
    death_watchers: Mutex<Vec<Arc<Mutex<DeathQueue>>>>,
}

#[derive(Debug, Default)]
struct DeathQueue {
    queue: VecDeque<EndpointId>,
    waker: Option<Waker>,
}

/// Report `endpoint` dead to every outstanding watch.
fn report_death(shared: &Shared, endpoint: EndpointId) {
    let watchers: Vec<_> = shared
        .death_watchers
        .lock()
        .expect("death watchers poisoned")
        .clone();
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

/// Watch one connection's death (decision 27): when `closed()` resolves
/// with anything but a close *we* initiated, the peer endpoint is reported
/// dead and the stale cache entry evicted. A `LocallyClosed` is cache
/// maintenance, not evidence — the peer is not dead because we hung up.
fn watch_connection(
    shared: Arc<Shared>,
    connection: iroh::endpoint::Connection,
    origin: EndpointId,
) {
    tokio::spawn(async move {
        let reason = connection.closed().await;
        // The suppression check and the report happen under one hold of the
        // connections lock, so a reconnect cannot complete between them and
        // let a stale report slip out (review nit, 2026-08-20; sound because
        // report_death takes only the watcher locks and never awaits).
        let mut connections = shared.connections.lock().await;
        let superseded = match connections.get(&origin) {
            // The cache still holds THIS connection: evict it, and the
            // death is current.
            Some(cached) if cached.stable_id() == connection.stable_id() => {
                connections.remove(&origin);
                false
            }
            // A NEWER connection to the same peer exists — a re-dial
            // already succeeded, so the peer is demonstrably not dead.
            // This report is stale (the reviewer's story-2 hazard 1: a
            // death report racing the reconnect the glossary promises),
            // and the transport is the one layer that can tell, because
            // connection generations are visible here. Suppress it.
            Some(_) => true,
            None => false,
        };
        if !superseded && !matches!(reason, ConnectionError::LocallyClosed) {
            report_death(&shared, origin);
        }
        drop(connections);
    });
}

/// One kamiroh endpoint on the Iroh network: owns the iroh `Endpoint`, the
/// local actor router, and the accept loop. Clone handles freely.
#[derive(Clone)]
pub struct IrohNet {
    shared: Arc<Shared>,
    /// Held by the founding handle set; aborting ends the accept loop.
    accept_loop: Arc<AbortOnDrop>,
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// How an endpoint meets the network (`ARCHITECTURE.md`, decision 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetProfile {
    /// Relay-less and lookup-less: static peer book only
    /// (`presets::Minimal` + `RelayMode::Disabled`). The default — tests,
    /// closed deployments, anything hermetic.
    #[default]
    Hermetic,
    /// n0's public infrastructure (`presets::N0`): relay fleet for
    /// rendezvous/fallback, address publishing + lookup so peers dial by
    /// endpoint id alone. NATs — however many layers — are Iroh's problem,
    /// not the operator's.
    N0,
}

impl IrohNet {
    /// Bind a [`NetProfile::Hermetic`] endpoint from domain [`Secret`] key
    /// material (32 bytes) and start the accept loop.
    pub async fn bind(secret: &Secret) -> Result<Self, IrohNetError> {
        Self::bind_inner(secret, NetProfile::Hermetic, None).await
    }

    /// Like [`IrohNet::bind`], but listening on a fixed UDP port — for
    /// relay-less endpoints that must be dialable at a pre-arranged address
    /// (a port-forwarded router, a container with a published port). Under
    /// [`NetProfile::N0`] a fixed port is unnecessary: dial by id instead.
    pub async fn bind_on(secret: &Secret, port: Option<u16>) -> Result<Self, IrohNetError> {
        Self::bind_inner(secret, NetProfile::Hermetic, port).await
    }

    /// Bind with an explicit [`NetProfile`].
    pub async fn bind_with(secret: &Secret, profile: NetProfile) -> Result<Self, IrohNetError> {
        Self::bind_inner(secret, profile, None).await
    }

    async fn bind_inner(
        secret: &Secret,
        profile: NetProfile,
        port: Option<u16>,
    ) -> Result<Self, IrohNetError> {
        let bytes: [u8; 32] = secret
            .expose()
            .try_into()
            .map_err(|_| IrohNetError::BadSecret)?;
        let secret_key = SecretKey::from_bytes(&bytes);
        let mut builder = match profile {
            NetProfile::Hermetic => {
                Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)
            }
            NetProfile::N0 => Endpoint::builder(presets::N0),
        };
        if let Some(port) = port {
            builder = builder
                .bind_addr(format!("0.0.0.0:{port}"))
                .map_err(|e| IrohNetError::Bind(e.to_string()))?;
        }
        let endpoint = builder
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| IrohNetError::Bind(e.to_string()))?;

        let endpoint_id = node_id_to_endpoint_id(endpoint.id());
        let shared = Arc::new(Shared {
            endpoint,
            endpoint_id,
            router: Mutex::new(Router::default()),
            peers: Mutex::new(HashMap::new()),
            connections: tokio::sync::Mutex::new(HashMap::new()),
            death_watchers: Mutex::new(Vec::new()),
        });

        let accept_shared = Arc::clone(&shared);
        let accept_loop = tokio::spawn(async move {
            accept_loop(accept_shared).await;
        });

        Ok(Self {
            shared,
            accept_loop: Arc::new(AbortOnDrop(accept_loop)),
        })
    }

    /// This endpoint's domain identity.
    pub fn endpoint_id(&self) -> &EndpointId {
        &self.shared.endpoint_id
    }

    /// This endpoint's dialable address, for handing to peers' `add_peer`.
    pub async fn addr(&self) -> Result<NodeAddr, IrohNetError> {
        // iroh 1.0: `addr()` is a plain getter over a watcher whose direct
        // addresses populate shortly after bind. Wait for the first non-empty
        // set so the peer book never caches an undialable address.
        let mut watcher = self.shared.endpoint.watch_addr();
        loop {
            let addr = watcher.get();
            if !addr.addrs.is_empty() {
                return Ok(addr);
            }
            watcher
                .updated()
                .await
                .map_err(|e| IrohNetError::Addr(format!("{e:?}")))?;
        }
    }

    /// Introduce a peer by endpoint id alone — usable under
    /// [`NetProfile::N0`], where address lookup resolves the id to a path
    /// (relay and, when hole-punching succeeds, direct).
    pub fn add_peer_by_id(&self, id: &EndpointId) -> Result<(), IrohNetError> {
        let node_id = endpoint_id_to_node_id(id)?;
        self.add_peer(NodeAddr::new(node_id));
        Ok(())
    }

    /// A one-line description of the live network paths to `peer`, if a
    /// connection exists — diagnostic sugar for checks ("did hole-punching
    /// win, or are we relaying?").
    pub async fn paths_to(&self, peer: &EndpointId) -> Option<String> {
        let connections = self.shared.connections.lock().await;
        connections.get(peer).map(|c| format!("{:?}", c.paths()))
    }

    /// Introduce a peer: static addressing, per the deferred-discovery
    /// decision. Returns the peer's domain endpoint id.
    pub fn add_peer(&self, addr: NodeAddr) -> EndpointId {
        let id = node_id_to_endpoint_id(addr.id);
        self.shared
            .peers
            .lock()
            .expect("peers poisoned")
            .insert(id.clone(), addr);
        id
    }

    /// A [`DeathWatch`] onto the network (decision 27): reports peers whose
    /// connections closed by *their* doing — application close, timeout,
    /// reset — never closes this side initiated.
    pub fn death_watch(&self) -> IrohDeathWatch {
        let queue = Arc::new(Mutex::new(DeathQueue::default()));
        self.shared
            .death_watchers
            .lock()
            .expect("death watchers poisoned")
            .push(Arc::clone(&queue));
        IrohDeathWatch { queue }
    }

    /// Deliberately close the cached connection to `peer`, if any — cache
    /// maintenance (a fresh dial replaces it on the next send), and a test
    /// lever for the "a conversation spans connections" promise. This side
    /// sees `LocallyClosed` (not a death); the *peer* sees an application
    /// close, which is death evidence there and fails its live exchanges
    /// with us — deliberate hang-ups are how endpoints say goodbye.
    pub async fn close_connection(&self, peer: &EndpointId) -> bool {
        let removed = self.shared.connections.lock().await.remove(peer);
        match removed {
            Some(connection) => {
                connection.close(0u32.into(), b"kamiroh: connection closed by this side");
                true
            }
            None => false,
        }
    }

    /// Close this endpoint entirely: every peer's `closed()` fires (death
    /// evidence on their side), and this net sends and receives nothing
    /// more. The graceful goodbye a killed container never says.
    pub async fn shutdown(&self) {
        self.shared.endpoint.close().await;
    }

    /// A [`Transport`] handle onto the network.
    pub fn transport(&self) -> IrohTransport {
        IrohTransport {
            shared: Arc::clone(&self.shared),
            _accept_loop: Arc::clone(&self.accept_loop),
        }
    }
}

impl Registry for IrohNet {
    type Inbox = IrohInbox;
    type Error = IrohNetError;

    fn bind(&mut self, address: &Address) -> Result<Self::Inbox, Self::Error> {
        if address.endpoint != self.shared.endpoint_id {
            return Err(IrohNetError::WrongEndpoint);
        }
        let mut router = self.shared.router.lock().expect("router poisoned");
        if let Some(existing) = router.bound.get(&address.name)
            && !existing.is_closed()
        {
            return Err(IrohNetError::NameInUse);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        router.bound.insert(address.name.clone(), tx);
        Ok(IrohInbox { rx })
    }
}

/// [`Inbox`] over the accept loop's routing. Dropping it closes the channel;
/// the router prunes the binding lazily (a closed sender may be rebound).
pub struct IrohInbox {
    rx: mpsc::UnboundedReceiver<Delivery>,
}

impl Inbox for IrohInbox {
    async fn next(&mut self) -> Option<Delivery> {
        self.rx.recv().await
    }
}

/// [`Transport`] implementation: one postcard frame per uni-stream, cached
/// connections, one retry on stale connections.
#[derive(Clone)]
pub struct IrohTransport {
    shared: Arc<Shared>,
    _accept_loop: Arc<AbortOnDrop>,
}

impl Transport for IrohTransport {
    type Error = IrohTransportError;

    async fn send(
        &mut self,
        from: &Address,
        to: &Address,
        message: Message,
    ) -> Result<(), Self::Error> {
        if from.endpoint != self.shared.endpoint_id {
            return Err(IrohTransportError::NotOurEndpoint);
        }
        let frame = Frame::Single {
            from_name: from.name.clone(),
            to_name: to.name.clone(),
            message,
        };
        self.send_encoded(&to.endpoint, &frame).await
    }

    async fn open_many(
        &mut self,
        from: &Address,
        to_endpoint: &EndpointId,
        to_names: &[ActorName],
        request: Request,
    ) -> Result<(), Self::Error> {
        if from.endpoint != self.shared.endpoint_id {
            return Err(IrohTransportError::NotOurEndpoint);
        }
        let frame = Frame::OpenMany {
            from_name: from.name.clone(),
            to_names: to_names.to_vec(),
            request,
        };
        self.send_encoded(to_endpoint, &frame).await
    }
}

impl IrohTransport {
    /// Encode `frame` and send it with one stale-connection retry — the
    /// shared tail of both [`Transport`] methods.
    async fn send_encoded(
        &self,
        to_endpoint: &EndpointId,
        frame: &Frame,
    ) -> Result<(), IrohTransportError> {
        let bytes =
            postcard::to_stdvec(frame).map_err(|e| IrohTransportError::Encode(e.to_string()))?;
        // One retry: a cached connection may have gone stale.
        let mut last_err = None;
        for attempt in 0..2 {
            let connection = self.connection_to(to_endpoint, attempt > 0).await?;
            match send_frame(&connection, &bytes).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.expect("two attempts made"))
    }

    async fn connection_to(
        &self,
        peer: &EndpointId,
        force_fresh: bool,
    ) -> Result<iroh::endpoint::Connection, IrohTransportError> {
        let mut connections = self.shared.connections.lock().await;
        if !force_fresh && let Some(existing) = connections.get(peer) {
            return Ok(existing.clone());
        }
        let addr = self
            .shared
            .peers
            .lock()
            .expect("peers poisoned")
            .get(peer)
            .cloned()
            .ok_or(IrohTransportError::UnknownPeer)?;
        let connection = self
            .shared
            .endpoint
            .connect(addr, ALPN)
            .await
            .map_err(|e| IrohTransportError::Connect(e.to_string()))?;
        connections.insert(peer.clone(), connection.clone());
        // Streams the peer opens on THIS connection (e.g. replies) arrive
        // here, not at the accept loop — every connection gets a reader,
        // whichever side dialed it.
        spawn_reader(Arc::clone(&self.shared), connection.clone());
        // And every connection gets a death watch (decision 27).
        watch_connection(Arc::clone(&self.shared), connection.clone(), peer.clone());
        Ok(connection)
    }
}

async fn send_frame(
    connection: &iroh::endpoint::Connection,
    bytes: &[u8],
) -> Result<(), IrohTransportError> {
    let mut stream = connection
        .open_uni()
        .await
        .map_err(|e| IrohTransportError::Stream(e.to_string()))?;
    stream
        .write_all(bytes)
        .await
        .map_err(|e| IrohTransportError::Stream(e.to_string()))?;
    // Assumption point 5: finish may be sync or async across versions.
    stream
        .finish()
        .map_err(|e| IrohTransportError::Stream(e.to_string()))?;
    // Give the peer a chance to read before the connection is dropped by a
    // short-lived sender: await stream close acknowledgment.
    let _ = stream.stopped().await;
    Ok(())
}

async fn accept_loop(shared: Arc<Shared>) {
    // Assumption point 6: accept() → Incoming → Connection.
    while let Some(incoming) = shared.endpoint.accept().await {
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            let Ok(connection) = incoming.await else {
                return;
            };
            // An inbound connection teaches us the peer: cache it so replies
            // flow back over the very connection the request arrived on. A
            // receiving endpoint therefore needs no peer-book entry for its
            // callers — admission still gates every delivery at the app
            // layer. (Found by the Incus-check rehearsal: without this, a
            // server could hear but never answer an unknown-address caller.)
            let origin = node_id_to_endpoint_id(connection.remote_id());
            shared
                .connections
                .lock()
                .await
                .insert(origin.clone(), connection.clone());
            watch_connection(Arc::clone(&shared), connection.clone(), origin);
            spawn_reader(shared, connection);
        });
    }
}

/// Read frames off one connection for its lifetime, routing deliveries to
/// bound actors. Spawned for every connection — accepted *or* dialed —
/// because QUIC is bidirectional: the peer may open streams on either.
fn spawn_reader(shared: Arc<Shared>, connection: iroh::endpoint::Connection) {
    tokio::spawn(async move {
        // The proven origin: the connection's authenticated remote key.
        // iroh 1.0: `remote_id()` on an established Connection is infallible.
        let origin = node_id_to_endpoint_id(connection.remote_id());
        loop {
            let Ok(mut stream) = connection.accept_uni().await else {
                return; // connection closed
            };
            let Ok(bytes) = stream.read_to_end(MAX_FRAME_BYTES).await else {
                continue;
            };
            let Ok(frame) = postcard::from_bytes::<Frame>(&bytes) else {
                continue; // malformed frame: drop
            };
            match frame {
                Frame::Single {
                    from_name,
                    to_name,
                    message,
                } => {
                    let delivery = Delivery {
                        from: Address::new(origin.clone(), from_name),
                        to: Address::new(shared.endpoint_id.clone(), to_name),
                        message,
                    };
                    let router = shared.router.lock().expect("router poisoned");
                    if let Some(tx) = router.bound.get(&delivery.to.name) {
                        // Unknown or closed bindings drop silently: an unbound
                        // name discloses nothing.
                        let _ = tx.send(delivery);
                    }
                }
                Frame::OpenMany {
                    from_name,
                    to_names,
                    request,
                } => {
                    // The batch receipt goes back FIRST, at endpoint
                    // arrival, before any name is consulted — and even if
                    // every name below is unbound (decision 29): it
                    // promises arrival, and the names were never part of
                    // that promise. Best-effort over the same connection.
                    let receipt = Frame::Single {
                        from_name: batch_receipt_sender(&shared.endpoint_id).name,
                        to_name: from_name.clone(),
                        message: Message::Ack(Ack { id: request.id }),
                    };
                    if let Ok(receipt_bytes) = postcard::to_stdvec(&receipt) {
                        let _ = send_frame(&connection, &receipt_bytes).await;
                    }
                    let from = Address::new(origin.clone(), from_name);
                    let router = shared.router.lock().expect("router poisoned");
                    for name in to_names {
                        // Unbound names drop silently: an unbound name
                        // discloses nothing (decision 29 — a per-name
                        // error would be a roster oracle).
                        if let Some(tx) = router.bound.get(&name) {
                            let _ = tx.send(Delivery {
                                from: from.clone(),
                                to: Address::new(shared.endpoint_id.clone(), name.clone()),
                                message: Message::Turn(Turn::Open {
                                    request: request.clone(),
                                }),
                            });
                        }
                    }
                }
            }
        }
    });
}

/// [`DeathWatch`] over [`IrohNet`]: yields peers whose connections died by
/// the peer's doing, in the order the deaths were observed. Never closes.
pub struct IrohDeathWatch {
    queue: Arc<Mutex<DeathQueue>>,
}

impl DeathWatch for IrohDeathWatch {
    async fn next_death(&mut self) -> Option<EndpointId> {
        std::future::poll_fn(|cx| {
            let mut q = self.queue.lock().expect("death queue poisoned");
            match q.queue.pop_front() {
                Some(endpoint) => std::task::Poll::Ready(Some(endpoint)),
                None => {
                    q.waker = Some(cx.waker().clone());
                    std::task::Poll::Pending
                }
            }
        })
        .await
    }
}

fn node_id_to_endpoint_id(id: NodeId) -> EndpointId {
    EndpointId::new(Hex::new(format!("{id}")).expect("node id displays as hex"))
}

/// Convert a domain endpoint id back to an iroh node id (peer-book helper).
pub fn endpoint_id_to_node_id(id: &EndpointId) -> Result<NodeId, IrohNetError> {
    NodeId::from_str(id.as_hex().as_str()).map_err(|_| IrohNetError::BadEndpointId)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrohNetError {
    /// Secrets must be exactly 32 bytes of key material.
    BadSecret,
    /// An endpoint id that is not a valid public key.
    BadEndpointId,
    /// Binding the iroh endpoint failed.
    Bind(String),
    /// Obtaining our dialable address failed.
    Addr(String),
    /// The address belongs to a different endpoint than this net.
    WrongEndpoint,
    /// An actor with this name is already bound.
    NameInUse,
}

impl fmt::Display for IrohNetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrohNetError::BadSecret => f.write_str("secret must be 32 bytes of key material"),
            IrohNetError::BadEndpointId => f.write_str("endpoint id is not a valid public key"),
            IrohNetError::Bind(e) => write!(f, "binding endpoint failed: {e}"),
            IrohNetError::Addr(e) => write!(f, "obtaining endpoint address failed: {e}"),
            IrohNetError::WrongEndpoint => {
                f.write_str("address belongs to a different endpoint than this net")
            }
            IrohNetError::NameInUse => f.write_str("an actor with this name is already bound"),
        }
    }
}

impl std::error::Error for IrohNetError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrohTransportError {
    /// `from` must be an address on this endpoint.
    NotOurEndpoint,
    /// The target endpoint is not in the peer book.
    UnknownPeer,
    Encode(String),
    Connect(String),
    Stream(String),
}

impl fmt::Display for IrohTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrohTransportError::NotOurEndpoint => {
                f.write_str("from-address is not on this endpoint")
            }
            IrohTransportError::UnknownPeer => {
                f.write_str("target endpoint is not in the peer book")
            }
            IrohTransportError::Encode(e) => write!(f, "frame encoding failed: {e}"),
            IrohTransportError::Connect(e) => write!(f, "connecting failed: {e}"),
            IrohTransportError::Stream(e) => write!(f, "stream failed: {e}"),
        }
    }
}

impl std::error::Error for IrohTransportError {}
