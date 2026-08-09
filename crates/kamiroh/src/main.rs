//! kamiroh composition root.
//!
//! The only crate that names concrete adapters. It resolves each port to one
//! implementation, hands the wiring to the application layer, and does nothing
//! else — no policy, no protocol, no agent logic.
//!
//! Key custody is real: the node's secret is generated from OS entropy and
//! persisted, so its identity survives a restart. Every other port is still
//! bound to an in-memory adapter, so this binary starts, reports its endpoint
//! id, and exercises the control path against itself.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use kamiroh_adapter_fs::FileKeyStore;
use kamiroh_adapter_iroh::endpoint_id_for;
use kamiroh_adapter_memory::{EchoController, InMemoryAllowlist, LoopbackTransport};
use kamiroh_app::ControlService;
use kamiroh_domain::{ActorName, ControlMessage, ControlReply, EndpointId, PeerAddress};
use kamiroh_ports::{
    AgentController, Allowlist, ControlApi, ControlApiError, KeyStore, Origin, Transport,
};

/// Overrides where the node secret lives. Useful for running several nodes on
/// one machine, and for tests that must not touch the real key.
const KEY_FILE_ENV: &str = "KAMIROH_KEY_FILE";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    eprintln!(
        "kamiroh: development wiring — key custody is real; transport, allowlist \
         and controller are still in-memory."
    );

    // --- Driven ports -------------------------------------------------------
    let key_path = match std::env::var_os(KEY_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => FileKeyStore::default_path()?,
    };
    let key_store: Arc<dyn KeyStore> = Arc::new(FileKeyStore::new(&key_path));
    let secret = key_store.load_or_create().await?;
    println!("key file:    {}", key_path.display());

    // The real ed25519 public key for this secret — the id a peer will see.
    let local_endpoint = endpoint_id_for(&secret);

    // Self-allow, so the loopback smoke path below is authorised. A real node's
    // allowlist is built from configured peers and does not contain itself.
    let allowlist: Arc<dyn Allowlist> =
        Arc::new(InMemoryAllowlist::with_endpoints([local_endpoint]));

    let agent = ActorName::new("agent")?;
    let controller: Arc<dyn AgentController> =
        Arc::new(EchoController::with_agents([agent.clone()]));

    // --- Application --------------------------------------------------------
    let control: Arc<dyn ControlApi> = Arc::new(ControlService::new(allowlist, controller));

    // --- Fronts -------------------------------------------------------------
    // One front today. The Iroh and Herdr adapters plug into the same
    // `Arc<dyn ControlApi>`, which is how several fronts reach one controller.
    let transport: Arc<dyn Transport> =
        Arc::new(LoopbackTransport::new(local_endpoint, control.clone()));

    println!("endpoint id: {}", transport.local_endpoint_id());
    println!("agent:       {agent}");

    smoke_test(transport.as_ref(), control.as_ref(), &agent).await?;
    Ok(())
}

/// Drives the wiring once so a run proves the path end to end, in both the
/// allowed and the refused direction.
async fn smoke_test(
    transport: &dyn Transport,
    control: &dyn ControlApi,
    agent: &ActorName,
) -> Result<(), Box<dyn Error>> {
    let address = PeerAddress::new(transport.local_endpoint_id(), agent.clone());

    match transport
        .send(
            &address,
            ControlMessage::Prompt(kamiroh_domain::Payload::text("hello")),
        )
        .await?
    {
        ControlReply::Output(payload) => {
            println!("prompt -> {:?}", payload.as_text().unwrap_or("<binary>"));
        }
        other => println!("prompt -> {other:?}"),
    }

    // The same path from an endpoint that is not on the allowlist must be
    // refused before it reaches the agent.
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
