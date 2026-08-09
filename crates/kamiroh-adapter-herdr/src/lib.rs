//! The Herdr-pane console.
//!
//! One pane, one agent, typed at like a chat window. A Herdr pane already means
//! "one agent" to the person using it, so this crate takes that as its shape:
//! a console is *bound* to an agent when it starts, and no line ever has to say
//! which agent it means.
//!
//! # It is a console, not only a front
//!
//! The build plan describes slice J as "a second front calling the same
//! `ControlApi`", and half of that is right. But the case worth having is the
//! other direction: sitting at a pane on a laptop and driving a long-running
//! agent on the home node, across the network. That is not a front at all —
//! nothing arrives — it is kamiroh acting as a *client* of a peer, over the
//! `Transport` port.
//!
//! Both directions are here, behind [`Link`], and the console cannot tell them
//! apart:
//!
//! - [`LocalLink`] calls `ControlApi` with `Origin::local_front()`. This is the
//!   "second front" the architecture has claimed since slice A, and the first
//!   thing to actually test it: it holds the same `Arc<dyn ControlApi>` as the
//!   Iroh front, so both reach one controller actor.
//! - [`RemoteLink`] calls `Transport`, so the agent may be anywhere the
//!   allowlist at the far end permits.
//!
//! # The console does not know about Herdr; the reporter does
//!
//! A pane is a terminal: input arrives on stdin, output goes to stdout. That is
//! the entire surface [`console`] needs, which is why it takes an
//! `AsyncBufRead` and an `AsyncWrite` and is tested with a string and a
//! `Vec<u8>`.
//!
//! [`report`] is the half that does know. Herdr keeps a state per pane, and
//! [`report::attach`] wraps a [`Link`] so that driving an agent updates it —
//! `working` while a prompt runs, `idle` when it lands. It speaks Herdr's local
//! socket API ([`client`]): newline-delimited JSON on `$HERDR_SOCKET_PATH`,
//! method `pane.report_agent`.
//!
//! Outside a pane, `attach` returns the link untouched. kamiroh runs outside
//! Herdr as a matter of course, and reporting is never allowed to delay a
//! control message or fail one.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use kamiroh_adapter_herdr::{LocalLink, console};
//! use kamiroh_domain::ActorName;
//! use kamiroh_ports::ControlApi;
//!
//! # async fn wire(control: Arc<dyn ControlApi>) -> std::io::Result<()> {
//! let agent = ActorName::new("agent").unwrap();
//! let link = Arc::new(LocalLink::new(control, agent));
//!
//! console::serve(
//!     tokio::io::BufReader::new(tokio::io::stdin()),
//!     tokio::io::stdout(),
//!     link,
//!     "> ",
//! )
//! .await
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// The Herdr-managed agent. Unix only, like the socket it drives.
#[cfg(unix)]
pub mod agent;
pub mod console;
pub mod link;
pub mod pane;
pub mod report;

/// Herdr's local socket API. Unix only — elsewhere it is a named pipe.
#[cfg(unix)]
pub mod client;

#[cfg(unix)]
pub use agent::HerdrAgent;
pub use link::{Link, LinkError, LocalLink, RemoteLink};
pub use pane::{Pane, PaneAgentState};
pub use report::ReportingLink;

/// Builds an agent driving the Herdr agent named by `target`, if one is
/// possible in this process.
///
/// `target` is a pane id (`w1:p2`) or an agent name, as Herdr accepts either.
/// Returns `None` where there is no socket to reach, or on a platform whose
/// Herdr socket is a named pipe this crate does not speak — so a caller can
/// fall back without knowing which of those it was.
pub fn herdr_agent(target: &str) -> Option<std::sync::Arc<dyn kamiroh_ports::Agent>> {
    #[cfg(unix)]
    {
        let socket = pane::socket_path()?;
        Some(std::sync::Arc::new(agent::HerdrAgent::new(
            client::Client::new(socket),
            target,
        )))
    }
    #[cfg(not(unix))]
    {
        let _ = target;
        None
    }
}
