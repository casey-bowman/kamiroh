//! kamiroh composition root.
//!
//! The only crate that names concrete adapters. It resolves each port to one
//! implementation, hands the wiring to the application layer, and does nothing
//! else — no policy, no protocol, no agent logic.
//!
//! As of slice G this is a real node: it holds a persistent identity, listens
//! for allowlisted peers over Iroh, and drives agents — local or remote — that
//! are real Kameo actors. What those agents *do* is still the echo stand-in,
//! and the allowlist is read from the environment rather than from a config
//! adapter (slice I).
//!
//! # Configuration
//!
//! | Variable | Meaning |
//! |---|---|
//! | `KAMIROH_KEY_FILE` | Where the node secret lives. Default `$XDG_CONFIG_HOME/kamiroh/node.key`. |
//! | `KAMIROH_ALLOW` | Comma-separated peer endpoint ids (hex) permitted to reach this node. **Unset means deny everyone.** |
//! | `KAMIROH_PEER` | `<endpoint-id-hex>@<host:port>` — a peer to greet on startup. |

use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use kamiroh_adapter_fs::FileKeyStore;
use kamiroh_adapter_iroh::{
    EndpointAddr, IrohTransport, bind_endpoint, endpoint_id_for, front, peer_address,
};
use kamiroh_adapter_kameo::{EchoAgent, KameoController};
use kamiroh_adapter_memory::InMemoryAllowlist;
use kamiroh_app::ControlService;
use kamiroh_domain::{ActorName, ControlMessage, EndpointId, Payload, PeerAddress};
use kamiroh_ports::{
    AgentController, Allowlist, ControlApi, ControlApiError, KeyStore, Origin, Transport,
};

/// Where the node secret lives.
const KEY_FILE_ENV: &str = "KAMIROH_KEY_FILE";
/// Peers permitted to reach this node.
const ALLOW_ENV: &str = "KAMIROH_ALLOW";
/// A peer to greet on startup, as `<endpoint-id-hex>@<host:port>`.
const PEER_ENV: &str = "KAMIROH_PEER";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // --- Driven ports -------------------------------------------------------
    let key_path = match std::env::var_os(KEY_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => FileKeyStore::default_path()?,
    };
    let key_store: Arc<dyn KeyStore> = Arc::new(FileKeyStore::new(&key_path));
    let secret = key_store.load_or_create().await?;
    let local_endpoint = endpoint_id_for(&secret);

    let allowed = parse_allowlist()?;
    let allowlist: Arc<dyn Allowlist> =
        Arc::new(InMemoryAllowlist::with_endpoints(allowed.clone()));

    let agent = ActorName::new("agent")?;
    let controller: Arc<dyn AgentController> =
        Arc::new(KameoController::new().with_agent(agent.clone(), EchoAgent));

    // --- Application --------------------------------------------------------
    let control: Arc<dyn ControlApi> = Arc::new(ControlService::new(allowlist, controller));

    // --- Fronts and transport ----------------------------------------------
    let endpoint = bind_endpoint(&secret).await?;
    let listening: Vec<SocketAddr> = endpoint.bound_sockets();

    let peer = parse_peer()?;
    let transport: Arc<dyn Transport> = Arc::new(IrohTransport::new(
        endpoint.clone(),
        peer.iter().map(|(_, addr)| addr.clone()),
    ));

    // The Iroh front and the application share one `Arc<dyn ControlApi>`. A
    // Herdr front (slice J) will hold this same handle — that is what "several
    // fronts, one controller actor" means in code.
    tokio::spawn(front::serve(endpoint, Arc::clone(&control)));

    println!("key file:    {}", key_path.display());
    println!("endpoint id: {local_endpoint}");
    println!("listening:   {listening:?}");
    println!("agent:       {agent}");
    println!(
        "allowing:    {} peer(s){}",
        allowed.len(),
        if allowed.is_empty() {
            format!(" — set {ALLOW_ENV} to admit peers; nothing is admitted by default")
        } else {
            String::new()
        }
    );

    local_smoke(control.as_ref(), &agent).await?;

    if let Some((peer_id, _)) = peer {
        greet(transport.as_ref(), peer_id, &agent).await;
    }

    println!("\nserving — press Ctrl-C to stop");
    tokio::signal::ctrl_c().await?;
    println!("stopping");
    Ok(())
}

/// Reads the allowlist. Absent or empty means admit nobody.
fn parse_allowlist() -> Result<Vec<EndpointId>, Box<dyn Error>> {
    let Some(raw) = std::env::var_os(ALLOW_ENV) else {
        return Ok(Vec::new());
    };
    let raw = raw.to_string_lossy().into_owned();
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry.parse::<EndpointId>().map_err(|error| {
                format!("{ALLOW_ENV} entry {entry:?} is not an endpoint id: {error}").into()
            })
        })
        .collect()
}

/// Reads `KAMIROH_PEER` as `<endpoint-id-hex>@<host:port>`.
fn parse_peer() -> Result<Option<(EndpointId, EndpointAddr)>, Box<dyn Error>> {
    let Some(raw) = std::env::var_os(PEER_ENV) else {
        return Ok(None);
    };
    let raw = raw.to_string_lossy().into_owned();
    let (id, socket) = raw
        .split_once('@')
        .ok_or_else(|| format!("{PEER_ENV} must look like <endpoint-id-hex>@<host:port>"))?;

    let id: EndpointId = id.trim().parse()?;
    let socket: SocketAddr = socket.trim().parse()?;
    Ok(Some((id, peer_address(&id, socket)?)))
}

/// Proves the local path end to end, in both the allowed and refused direction.
///
/// `Origin::local_front` is the in-process front — the same trust position a
/// Herdr pane will occupy — so it reaches the agent without consulting the
/// allowlist. The refusal below goes the other way.
async fn local_smoke(control: &dyn ControlApi, agent: &ActorName) -> Result<(), Box<dyn Error>> {
    let reply = control
        .deliver(
            Origin::local_front(),
            agent,
            ControlMessage::Prompt(Payload::text("hello")),
        )
        .await?;
    println!("local prompt -> {reply:?}");

    let stranger = EndpointId::from_bytes([0xee; 32]);
    match control
        .deliver(Origin::remote(stranger), agent, ControlMessage::Status)
        .await
    {
        Err(ControlApiError::NotAllowed { endpoint }) => {
            println!("unlisted peer {endpoint} -> refused");
        }
        other => return Err(format!("unlisted peer was not refused: {other:?}").into()),
    }
    Ok(())
}

/// Says hello to a configured peer, reporting whatever comes back.
///
/// A refusal is a normal outcome worth seeing: it means the peer is reachable
/// but has not allowlisted this node.
async fn greet(transport: &dyn Transport, peer: EndpointId, agent: &ActorName) {
    let address = PeerAddress::new(peer, agent.clone());
    match transport
        .send(
            &address,
            ControlMessage::Prompt(Payload::text("hello, peer")),
        )
        .await
    {
        Ok(reply) => println!("peer {peer} -> {reply:?}"),
        Err(error) => println!("peer {peer} -> {error}"),
    }
}
