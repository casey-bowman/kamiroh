//! kamiroh composition root.
//!
//! The only crate that names concrete adapters. It resolves each port to one
//! implementation, hands the wiring to the application layer, and does nothing
//! else — no policy, no protocol, no agent logic.
//!
//! As of M1 every driven port has a real adapter and so does the agent itself:
//! a persistent identity on disk, an allowlist read from a file, an Iroh
//! transport and front, controller actors, and — with
//! `KAMIROH_AGENT_TARGET` set — a real coding agent that Herdr is managing.
//!
//! # Configuration
//!
//! | Variable | Meaning |
//! |---|---|
//! | `KAMIROH_KEY_FILE` | Where the node secret lives. Default `$XDG_CONFIG_HOME/kamiroh/node.key`. |
//! | `KAMIROH_ALLOW_FILE` | The allowlist file. Default `$XDG_CONFIG_HOME/kamiroh/allow`. |
//! | `KAMIROH_ALLOW` | Comma-separated endpoint ids. **Overrides the file entirely**, for tests and for running several nodes on one machine. |
//! | `KAMIROH_PEER` | `<endpoint-id-hex>` or `<endpoint-id-hex>@<host:port>` — a peer to greet and to bind the pane console to. The address is optional when the reach can look one up. |
//! | `KAMIROH_AGENT_TARGET` | A Herdr pane id or agent name to drive. Unset means the echo stand-in. |
//! | `KAMIROH_REACH` | `direct` (default) or `anywhere`. **`anywhere` publishes this node's addresses** to a public lookup service so peers can find it by endpoint id; read [`Reach`] before setting it. |
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

use kamiroh_adapter_fs::{AgentSpec, FileAllowlist, FileKeyStore, agents};
use kamiroh_adapter_herdr::{Link, LocalLink, RemoteLink, console, report};
use kamiroh_adapter_iroh::{
    EndpointAddr, IrohTransport, Reach, bind_endpoint, endpoint_id_for, front, peer_address,
};
use kamiroh_adapter_kameo::KameoController;
use kamiroh_adapter_memory::{EchoAgent, InMemoryAllowlist};
use kamiroh_app::ControlService;
use kamiroh_domain::{ActorName, ControlMessage, EndpointId, Payload, PeerAddress};
use kamiroh_ports::{
    Agent, AgentController, Allowlist, ControlApi, ControlApiError, KeyStore, Origin, Transport,
};

/// Where the node secret lives.
const KEY_FILE_ENV: &str = "KAMIROH_KEY_FILE";
/// Peers permitted to reach this node, overriding the allowlist file.
const ALLOW_ENV: &str = "KAMIROH_ALLOW";
/// Where the allowlist lives.
const ALLOW_FILE_ENV: &str = "KAMIROH_ALLOW_FILE";
/// A peer to greet on startup, as `<endpoint-id-hex>@<host:port>`.
const PEER_ENV: &str = "KAMIROH_PEER";
/// The Herdr-managed agent this node drives. Overrides the agents file.
const AGENT_TARGET_ENV: &str = "KAMIROH_AGENT_TARGET";
/// Which agents this node hosts.
const AGENTS_FILE_ENV: &str = "KAMIROH_AGENTS_FILE";
/// The agent name to address on the peer. Defaults to the pane's own agent.
const PEER_AGENT_ENV: &str = "KAMIROH_PEER_AGENT";
/// How far this node can be reached from: `direct` (default) or `anywhere`.
const REACH_ENV: &str = "KAMIROH_REACH";
/// Per-crate log filter, e.g. `kamiroh_adapter_iroh=debug,kamiroh_app=info`.
const LOG_ENV: &str = "KAMIROH_LOG";

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

/// Every kamiroh crate that emits, for per-crate filtering.
///
/// Listed rather than prefix-matched: `Targets` matches on `::`-separated path
/// segments, and `kamiroh_app` does not begin with the segment `kamiroh`.
const KAMIROH_TARGETS: [&str; 6] = [
    "kamiroh",
    "kamiroh_app",
    "kamiroh_adapter_fs",
    "kamiroh_adapter_iroh",
    "kamiroh_adapter_kameo",
    "kamiroh_adapter_herdr",
];

/// Sends diagnostics to **stderr**, and says what it will show.
///
/// Stderr is not a detail. Since slice J1 stdout belongs to the pane console —
/// it is where an agent's answers appear and where a person is typing — so a
/// log line on stdout would land in the middle of someone's conversation.
///
/// Dependencies are quiet by default. Iroh in particular has plenty to say at
/// `debug`, and a node that floods its own diagnostics is no more observable
/// than one that says nothing.
fn init_tracing() -> String {
    use tracing_subscriber::filter::Targets;
    use tracing_subscriber::prelude::*;

    let requested = std::env::var(LOG_ENV)
        .ok()
        .filter(|spec| !spec.trim().is_empty());

    let (filter, summary) = match requested.as_deref() {
        Some(spec) => match spec.parse::<Targets>() {
            Ok(targets) => (targets, format!("{spec} (from {LOG_ENV})")),
            // A bad filter must not silence the node. Falling back loudly beats
            // starting with logging quietly disabled by a typo.
            Err(error) => (
                default_targets(),
                format!(
                    "kamiroh=info, dependencies=warn — {LOG_ENV}={spec:?} is not a valid filter ({error})"
                ),
            ),
        },
        None => (
            default_targets(),
            format!("kamiroh=info, dependencies=warn — set {LOG_ENV} to change"),
        ),
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true),
        )
        .with(filter)
        .init();

    summary
}

/// kamiroh's own crates at `info`, everything else at `warn`.
fn default_targets() -> tracing_subscriber::filter::Targets {
    use tracing_subscriber::filter::{LevelFilter, Targets};

    KAMIROH_TARGETS.iter().fold(
        Targets::new().with_default(LevelFilter::WARN),
        |targets, crate_name| targets.with_target(*crate_name, LevelFilter::INFO),
    )
}

async fn run() -> Result<(), Box<dyn Error>> {
    let logging = init_tracing();
    // --- Driven ports -------------------------------------------------------
    let key_path = match std::env::var_os(KEY_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => FileKeyStore::default_path()?,
    };
    let key_store: Arc<dyn KeyStore> = Arc::new(FileKeyStore::new(&key_path));
    let secret = key_store.load_or_create().await?;
    let local_endpoint = endpoint_id_for(&secret);

    let allowlists = build_allowlist()?;
    let allow_summary = allowlists.summary.clone();
    let allowlist = Arc::clone(&allowlists.port);
    if let Some(reloadable) = allowlists.reloadable.clone() {
        reload_allowlist_on_hangup(reloadable);
    }

    let hosted = build_agents()?;
    // The pane binds to the first agent. One pane means one agent (slice J1),
    // and *which* one has to be decided somewhere; the file's order is the
    // operator's own statement of what matters most.
    let agent = hosted
        .first()
        .map(|spec| spec.name.clone())
        .unwrap_or(ActorName::new(agents::DEFAULT_AGENT)?);
    let (controller, agent_summary) = build_controller(&hosted);

    // One reporter, fed from both directions. The console reports what a person
    // at this pane does; the controller reports what *anyone* does, which is
    // the half a serving node needs — its pane has nobody at it and the work
    // arrives through the Iroh front.
    let reporter = report::Reporter::start(&agent);
    let controller = match &reporter {
        // Reports are filtered to the pane's own agent: a pane shows one agent,
        // so letting every hosted agent report would make them overwrite each
        // other in Herdr's list rather than tell anyone anything.
        Some((reporter, _)) => reporter.wrap_controller(controller, agent.clone()),
        None => controller,
    };

    // --- Application --------------------------------------------------------
    let control: Arc<dyn ControlApi> = Arc::new(ControlService::new(allowlist, controller));

    // --- Fronts and transport ----------------------------------------------
    let reach = parse_reach()?;
    let endpoint = bind_endpoint(&secret, reach).await?;
    let listening: Vec<SocketAddr> = endpoint.bound_sockets();

    let peer = parse_peer()?;
    let transport: Arc<dyn Transport> = Arc::new(IrohTransport::new(
        endpoint.clone(),
        peer.iter().filter_map(|(_, addr)| addr.clone()),
        reach,
    ));

    // The Iroh front and the application share one `Arc<dyn ControlApi>`. A
    // Herdr front (slice J) will hold this same handle — that is what "several
    // fronts, one controller actor" means in code.
    tokio::spawn(front::serve(endpoint, Arc::clone(&control)));

    println!("key file:    {}", key_path.display());
    println!("endpoint id: {local_endpoint}");
    println!("listening:   {listening:?}");
    println!("reach:       {}", reach.describe());
    println!("logging:     {logging} (to stderr)");
    println!("agents:      {agent_summary}");
    println!("pane agent:  {agent}");
    println!("allowing:    {allow_summary}");

    local_smoke(control.as_ref(), &agent).await?;

    // The greet must probe the agent the pane will drive, not this node's own
    // name for one. Otherwise a pane bound to a peer's `reviewer` reports a
    // healthy greet against its `agent`, which is a reachability check for a
    // different thing than the one you are about to use.
    let peer_agent = peer_agent(&agent)?;
    if let Some((peer_id, _)) = &peer {
        // Spawned, not awaited. An unreachable peer takes the full dial timeout
        // — 16 seconds, measured — and awaiting it here holds up the pane
        // console, which is the thing a person is actually waiting for. A
        // laptop whose home node is asleep would look hung rather than offering
        // a prompt where `/status` explains the problem.
        let transport = Arc::clone(&transport);
        let target = peer_agent.clone();
        let peer_id = *peer_id;
        tokio::spawn(async move { greet(transport.as_ref(), peer_id, &target).await });
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
            // The peer names its own agents, and need not use ours.
            PeerAddress::new(*peer_id, peer_agent.clone()),
        )),
        None => Arc::new(LocalLink::new(Arc::clone(&control), agent.clone())),
    };
    // Herdr keeps a state per pane. Wrapping the link is what keeps that state
    // honest for a *remote* agent too: the messages never touch this node's
    // controller, so nothing downstream of it could report them.
    let (link, herdr) = match &reporter {
        Some((reporter, summary)) => (reporter.wrap_link(link), summary.clone()),
        None => (link, "not in a Herdr pane; not reporting".to_owned()),
    };
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

/// Resolves what is behind the agent, and a line saying which it is.
///
/// `KAMIROH_AGENT_TARGET` names a Herdr-managed agent — a pane id or an agent
/// name. Without it the echo stand-in runs, which is what makes a node useful
/// for testing the rest of kamiroh without a coding agent attached.
///
/// A target that is set but unusable falls back to the stand-in *and says so*.
/// Refusing to start would be defensible, but unlike a malformed allowlist this
/// is not a security boundary: the wrong answer here is a useless agent, not an
/// admitted stranger.
fn build_controller(hosted: &[AgentSpec]) -> (Arc<dyn AgentController>, String) {
    let mut controller = KameoController::new();
    let mut described = Vec::new();

    for spec in hosted {
        // `echo` and an unreachable Herdr socket both land on the stand-in, but
        // they are said differently: one was asked for, the other is a
        // consolation, and an operator needs to tell those apart.
        let (agent, description): (Arc<dyn Agent>, String) = if spec.is_echo() {
            (Arc::new(EchoAgent), format!("{} = echo", spec.name))
        } else {
            match kamiroh_adapter_herdr::herdr_agent(&spec.target) {
                Some(herdr) => (herdr, format!("{} = {}", spec.name, spec.target)),
                None => (
                    Arc::new(EchoAgent),
                    format!("{} = echo (no Herdr socket for {})", spec.name, spec.target),
                ),
            }
        };
        controller.spawn(spec.name.clone(), agent);
        described.push(description);
    }

    let summary = if described.is_empty() {
        "none — this node hosts no agents".to_owned()
    } else {
        described.join(", ")
    };
    (Arc::new(controller), summary)
}

/// Which agents this node hosts.
///
/// `KAMIROH_AGENT_TARGET` overrides the file with a single agent named
/// `agent`, the same shape as `KAMIROH_ALLOW` overriding the allowlist: one
/// variable to run a node without writing config, which is what every demo and
/// test does.
fn build_agents() -> Result<Vec<AgentSpec>, Box<dyn Error>> {
    if let Some(target) = std::env::var_os(AGENT_TARGET_ENV)
        .map(|target| target.to_string_lossy().into_owned())
        .filter(|target| !target.trim().is_empty())
    {
        return Ok(vec![AgentSpec {
            name: ActorName::new(agents::DEFAULT_AGENT)?,
            target,
        }]);
    }

    let path = match std::env::var_os(AGENTS_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => agents::default_path()?,
    };
    Ok(agents::load(&path)?)
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
fn build_allowlist() -> Result<Allowlists, Box<dyn Error>> {
    if let Some(raw) = std::env::var_os(ALLOW_ENV) {
        let endpoints = parse_allow_env(&raw.to_string_lossy())?;
        let summary = match endpoints.len() {
            0 => format!("0 peer(s) — {ALLOW_ENV} is set but empty; nothing is admitted"),
            count => format!("{count} peer(s) from {ALLOW_ENV}"),
        };
        // An env-var allowlist has nothing to re-read, so it is not reloadable.
        return Ok(Allowlists {
            port: Arc::new(InMemoryAllowlist::with_endpoints(endpoints)),
            reloadable: None,
            summary,
        });
    }

    let path = match std::env::var_os(ALLOW_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => FileAllowlist::default_path()?,
    };
    let allowlist = Arc::new(FileAllowlist::load(&path)?);
    let summary = match allowlist.len() {
        0 => format!(
            "0 peer(s) — add endpoint ids to {}, one per line; nothing is admitted",
            path.display()
        ),
        count => format!("{count} peer(s) from {}", path.display()),
    };
    Ok(Allowlists {
        port: Arc::clone(&allowlist) as Arc<dyn Allowlist>,
        reloadable: Some(allowlist),
        summary,
    })
}

/// The allowlist, plus the handle that can re-read it.
///
/// The port is what everything uses; the concrete handle exists only so a
/// signal can call `reload`. Keeping both is the composition root's job — it is
/// the one place allowed to know which implementation it chose.
struct Allowlists {
    port: Arc<dyn Allowlist>,
    reloadable: Option<Arc<FileAllowlist>>,
    summary: String,
}

/// Re-reads the allowlist on `SIGHUP`, for as long as the node runs.
///
/// A signal rather than a console command, because the node that needs this is
/// the one with nobody at its pane: a home node serving peers, whose allowlist
/// is the thing an operator edits. `reload` keeps the previous set when the new
/// file is bad, so a fumbled edit costs a log line rather than every peer.
#[cfg(unix)]
fn reload_allowlist_on_hangup(allowlist: Arc<FileAllowlist>) {
    use tokio::signal::unix::{SignalKind, signal};

    tokio::spawn(async move {
        let mut hangup = match signal(SignalKind::hangup()) {
            Ok(hangup) => hangup,
            Err(error) => {
                tracing::warn!(%error, "cannot listen for SIGHUP; allowlist will not reload");
                return;
            }
        };

        while hangup.recv().await.is_some() {
            match allowlist.reload() {
                Ok(peers) => tracing::info!(
                    path = %allowlist.path().display(),
                    peers,
                    "allowlist reloaded on SIGHUP"
                ),
                // The previous set is still in force; say so, because "reload
                // failed" and "nobody is admitted now" are very different.
                Err(error) => tracing::warn!(
                    path = %allowlist.path().display(),
                    %error,
                    "allowlist reload failed; keeping the previous one"
                ),
            }
        }
    });
}

/// Non-Unix: no `SIGHUP`, so the allowlist is read once at startup.
#[cfg(not(unix))]
fn reload_allowlist_on_hangup(_allowlist: Arc<FileAllowlist>) {}

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

/// A peer to talk to: who it is, and where — when the where is known.
///
/// `None` for the address means "look it up", which only a reach that resolves
/// by id can do.
type Peer = (EndpointId, Option<EndpointAddr>);

/// Reads `KAMIROH_PEER` as `<endpoint-id-hex>` or `<endpoint-id-hex>@<host:port>`.
///
/// The address is optional since M2. Without one the peer is dialled by
/// identity alone, which only works when this node's reach can look addresses
/// up — and that is the whole point: from a cafe there is no `host:port` to
/// write down.
fn parse_peer() -> Result<Option<Peer>, Box<dyn Error>> {
    let Some(raw) = std::env::var_os(PEER_ENV) else {
        return Ok(None);
    };
    let raw = raw.to_string_lossy().into_owned();
    let raw = raw.trim();

    let Some((id, socket)) = raw.split_once('@') else {
        let id: EndpointId = raw.parse()?;
        return Ok(Some((id, None)));
    };

    let id: EndpointId = id.trim().parse()?;
    let socket: SocketAddr = socket.trim().parse()?;
    Ok(Some((id, Some(peer_address(&id, socket)?))))
}

/// Which agent to address on the peer.
///
/// Defaults to this pane's own agent name, which is right when both ends are
/// configured alike and wrong as soon as they are not — a home node hosting
/// `reviewer` cannot be driven by a laptop whose pane agent is `agent` without
/// saying so.
fn peer_agent(local: &ActorName) -> Result<ActorName, Box<dyn Error>> {
    match std::env::var_os(PEER_AGENT_ENV) {
        Some(name) => Ok(ActorName::new(name.to_string_lossy().trim())?),
        None => Ok(local.clone()),
    }
}

/// Reads `KAMIROH_REACH`. Absent means [`Reach::Direct`].
///
/// Opt-in rather than default, because `anywhere` publishes this node's
/// addresses to a public service. A node should not start announcing where it
/// lives because nobody set a variable.
fn parse_reach() -> Result<Reach, Box<dyn Error>> {
    let Some(raw) = std::env::var_os(REACH_ENV) else {
        return Ok(Reach::Direct);
    };
    match raw.to_string_lossy().trim().to_ascii_lowercase().as_str() {
        "direct" | "" => Ok(Reach::Direct),
        "anywhere" => Ok(Reach::Anywhere),
        other => Err(format!("{REACH_ENV} must be `direct` or `anywhere`, not {other:?}").into()),
    }
}

/// Proves the local path end to end, in both the allowed and refused direction.
///
/// `Origin::local_front` is the in-process front — the same trust position a
/// Herdr pane will occupy — so it reaches the agent without consulting the
/// allowlist. The refusal below goes the other way.
async fn local_smoke(control: &dyn ControlApi, agent: &ActorName) -> Result<(), Box<dyn Error>> {
    // `Status`, not `Prompt`. A prompt now reaches a real coding agent, so a
    // startup smoke that sent one would spend tokens on every launch and put
    // words in the agent's mouth that nobody asked for. `Status` proves the
    // same path — front, allowlist bypass, controller — and costs nothing.
    let reply = control
        .deliver(Origin::local_front(), agent, ControlMessage::Status)
        .await?;
    println!("local status -> {reply:?}");

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
