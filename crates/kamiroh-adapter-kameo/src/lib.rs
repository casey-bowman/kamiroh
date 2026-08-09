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
//! - `Detach` stops a real thing — this actor. Later messages are refused with
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
//! This crate drives [`Agent`](kamiroh_ports::Agent) but no longer defines it:
//! the trait moved to `kamiroh-ports` in M1, when a second adapter arrived to
//! implement it. `EchoAgent` moved with it, to the test-double crate.
//!
//! An agent's run does not have to finish. `AgentOutcome` carries where it
//! ended up, and the actor turns anything short of finished into
//! [`ControlReply::Partial`](kamiroh_domain::ControlReply::Partial) rather than
//! claiming an answer is complete.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use kamiroh_adapter_kameo::KameoController;
//! use kamiroh_adapter_memory::EchoAgent;
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

pub mod controller;

mod actor;

pub use controller::KameoController;
