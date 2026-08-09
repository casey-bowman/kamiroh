//! kamiroh composition root.
//!
//! The only crate that names concrete adapters. It resolves each port to one
//! implementation, hands the wiring to the application layer, and does nothing
//! else — no policy, no protocol, no agent logic.
//!
//! As of slice I every driven port has a real adapter: a persistent identity on
//! disk, an allowlist read from a file, an Iroh transport and front, and agents
//! that are real Kameo actors. What those agents *do* is still the echo
//! stand-in.
//!
//! # Configuration
//!
//! | Variable | Meaning |
//! |---|---|
//! | `KAMIROH_KEY_FILE` | Where the node secret lives. Default `$XDG_CONFIG_HOME/kamiroh/node.key`. |
//! | `KAMIROH_ALLOW_FILE` | The allowlist file. Default `$XDG_CONFIG_HOME/kamiroh/allow`. |
//! | `KAMIROH_ALLOW` | Comma-separated endpoint ids. **Overrides the file entirely**, for tests and for running several nodes on one machine. |
//! | `KAMIROH_PEER` | `<endpoint-id-hex>@<host:port>` — a peer to greet on startup. |
//!
//! Nothing is admitted by default, whichever source is used: an unset variable,
//! an absent file and an empty file all mean "admit nobody". A file that exists
//! but cannot be parsed is fatal rather than empty — see
//! [`kamiroh_adapter_fs::allowlist`].

use std::error::Error;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use kamiroh_adapter_fs::{FileAllowlist, FileKeyStore};
use kamiroh_adapter_herdr::{Link, LocalLink, RemoteLink, console, report};
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
/// Peers permitted to reach this node, overriding the allowlist file.
const ALLOW_ENV: &str = "KAMIROH_ALLOW";
/// Where the allowlist lives.
const ALLOW_FILE_ENV: &str = "KAMIROH_ALLOW_FILE";
/// A peer to greet on startup, as `<endpoint-id-hex>@<host:port>`.
const PEER_ENV: &str = "KAMIROH_PEER";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // A `Result`-returning `main` prints the error's `Debug`, which for
            // these is a struct dump rather than the sentence the error was
            // written to be. Refusing to start is only as useful as the reason
            // it gives, and a malformed allowlist is the case that has to read
            // well: the operator is looking for which line they broke.
            //
            // Only the top level is printed. Every error type here embeds its
            // source in its own message, so walking the chain would repeat it.
            eprintln!("kamiroh: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    // --- Driven ports -------------------------------------------------------
    let key_path = match std::env::var_os(KEY_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => FileKeyStore::default_path()?,
    };
    let key_store: Arc<dyn KeyStore> = Arc::new(FileKeyStore::new(&key_path));
    let secret = key_store.load_or_create().await?;
    let local_endpoint = endpoint_id_for(&secret);

    let (allowlist, allow_summary) = build_allowlist()?;

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
    println!("allowing:    {allow_summary}");

    local_smoke(control.as_ref(), &agent).await?;

    if let Some((peer_id, _)) = &peer {
        greet(transport.as_ref(), *peer_id, &agent).await;
    }

    // --- The pane console ---------------------------------------------------
    //
    // Bound to one agent, because a Herdr pane means one agent to the person
    // using it. Which *node* that agent is on is the link's business, not the
    // console's: with a peer configured the pane drives the agent over there,
    // which is the case worth having — a pane on a laptop, an agent that has
    // been running at home for a week.
    let link: Arc<dyn Link> = match &peer {
        Some((peer_id, _)) => Arc::new(RemoteLink::new(
            Arc::clone(&transport),
            PeerAddress::new(*peer_id, agent.clone()),
        )),
        None => Arc::new(LocalLink::new(Arc::clone(&control), agent.clone())),
    };
    // Herdr keeps a state per pane. Wrapping the link is what keeps that state
    // honest for a *remote* agent too: the messages never touch this node's
    // controller, so nothing downstream of it could report them.
    let (link, herdr) = report::attach(link, &agent);
    println!("pane:        {}", link.describe());
    println!("herdr:       {herdr}");

    // Spawned, and its ending is not the node's ending. A node serving agents
    // for other peers has nobody at its pane and may have a closed stdin from
    // the moment it starts; that must not shut it down.
    // A prompt only when someone is there to see it. Piped input echoes
    // nothing, so a prompt per line would stack on one line rather than
    // marking where a person is meant to type.
    let prompt = if std::io::stdin().is_terminal() {
        "> "
    } else {
        ""
    };

    tokio::spawn(async move {
        let input = tokio::io::BufReader::new(tokio::io::stdin());
        if let Err(error) = console::serve(input, tokio::io::stdout(), link, prompt).await {
            eprintln!("kamiroh: pane console stopped: {error}");
        }
    });

    println!("\nserving — press Ctrl-C to stop");
    tokio::signal::ctrl_c().await?;
    println!("stopping");
    Ok(())
}

/// Resolves the allowlist, returning it and a line describing where it came
/// from.
///
/// `KAMIROH_ALLOW` wins outright when set — explicit beats ambient, and a node
/// launched with one should not also be reading a file it never mentioned. The
/// summary is returned rather than printed so this stays a resolver, and so the
/// startup output always names the source: an allowlist that silently came from
/// somewhere other than where the operator was editing is the failure mode most
/// worth making impossible to miss.
fn build_allowlist() -> Result<(Arc<dyn Allowlist>, String), Box<dyn Error>> {
    if let Some(raw) = std::env::var_os(ALLOW_ENV) {
        let endpoints = parse_allow_env(&raw.to_string_lossy())?;
        let summary = match endpoints.len() {
            0 => format!("0 peer(s) — {ALLOW_ENV} is set but empty; nothing is admitted"),
            count => format!("{count} peer(s) from {ALLOW_ENV}"),
        };
        return Ok((
            Arc::new(InMemoryAllowlist::with_endpoints(endpoints)),
            summary,
        ));
    }

    let path = match std::env::var_os(ALLOW_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => FileAllowlist::default_path()?,
    };
    let allowlist = FileAllowlist::load(&path)?;
    let summary = match allowlist.len() {
        0 => format!(
            "0 peer(s) — add endpoint ids to {}, one per line; nothing is admitted",
            path.display()
        ),
        count => format!("{count} peer(s) from {}", path.display()),
    };
    Ok((Arc::new(allowlist), summary))
}

/// Parses the comma-separated override.
fn parse_allow_env(raw: &str) -> Result<Vec<EndpointId>, Box<dyn Error>> {
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
