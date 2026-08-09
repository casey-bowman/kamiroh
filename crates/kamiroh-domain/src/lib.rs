//! kamiroh domain types.
//!
//! The innermost ring of the hexagon: names, addresses, key material, and the
//! control vocabulary. This crate depends on nothing but `std` — no Iroh, no
//! Kameo, no Herdr, no async runtime. That is a non-negotiable architectural
//! constraint, not an accident of the current implementation; see
//! `docs/ARCHITECTURE.md`.

#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]

pub mod actor;
pub mod control;
pub mod endpoint;
pub mod secret;

pub(crate) mod hex;

pub use actor::{ActorName, InvalidActorName, PeerAddress};
pub use control::{AgentStatus, ControlMessage, ControlReply, Payload};
pub use endpoint::{EndpointId, ParseEndpointIdError};
pub use secret::{NodeSecret, ParseNodeSecretError};
