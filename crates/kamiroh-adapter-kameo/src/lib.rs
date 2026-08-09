//! Kameo-backed kamiroh adapters.
//!
//! One controller actor per agent, behind the [`AgentController`] port. This is
//! the adapter the port was written for: `kamiroh-adapter-memory`'s
//! `EchoController` kept agent state in a `HashMap` and simulated a lifecycle,
//! whereas here each agent is an actor with a mailbox that owns its own state.
//!
//! What that buys, concretely:
//!
//! - Messages to one agent are handled one at a time, so the state machine
//!   needs no lock and a completion cannot interleave with an interrupt.
//! - `Shutdown` stops a real thing. Later messages are refused with
//!   [`ControllerError::Stopped`](kamiroh_ports::ControllerError::Stopped).
//! - [`AgentStatus::Busy`](kamiroh_domain::AgentStatus::Busy) is reachable: a
//!   prompt runs as its own task, so the agent can be observed working and
//!   [`Interrupt`](kamiroh_domain::ControlMessage::Interrupt) has something to
//!   cancel.
//!
//! `kameo::Actor` appears nowhere above this crate — [`KameoController`] is the
//! only type the composition root names, and it is reached as
//! `Arc<dyn AgentController>` like every other driven port.
//!
//! # Agents
//!
//! [`Agent`] is the seam for the work itself. [`EchoAgent`] returns its prompt
//! and is the stand-in until a real agent runtime lands; the controller around
//! it is not a stand-in.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use kamiroh_adapter_kameo::{EchoAgent, KameoController};
//! use kamiroh_domain::ActorName;
//! use kamiroh_ports::AgentController;
//!
//! # async fn wire() -> Result<(), Box<dyn std::error::Error>> {
//! let agent = ActorName::new("agent")?;
//! let controller: Arc<dyn AgentController> =
//!     Arc::new(KameoController::new().with_agent(agent, EchoAgent));
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod agent;
pub mod controller;

mod actor;

pub use agent::{Agent, EchoAgent};
pub use controller::KameoController;
