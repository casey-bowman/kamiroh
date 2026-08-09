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
//! # Nothing here knows about Herdr
//!
//! A pane is a terminal: input arrives on stdin, output goes to stdout. That is
//! the entire integration surface this crate needs, which is why it takes an
//! `AsyncBufRead` and an `AsyncWrite` rather than naming Herdr anywhere.
//!
//! Herdr *does* have a socket API — newline-delimited JSON on
//! `$HERDR_SOCKET_PATH`, with `pane.report_agent` for pushing an agent's state
//! into the pane list. Reporting kamiroh's `AgentStatus` that way is a real
//! integration and a separate slice: it is outbound, it needs a JSON client,
//! and it is not a front. Keeping it out of here is what lets this crate be
//! tested with a string as its input and a `Vec<u8>` as its output.
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

pub mod console;
pub mod link;

pub use link::{Link, LinkError, LocalLink, RemoteLink};
