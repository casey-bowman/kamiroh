//! The kamiroh domain core.
//!
//! Pure and dependency-free: this crate knows nothing about Kameo, Iroh, async
//! runtimes, or serialization formats. It holds the model described in
//! `ARCHITECTURE.md` — addressing, secrets, the allowlist policy, and the
//! constrained, agent-agnostic vocabulary actors exchange.

pub mod actor;
pub mod allowlist;
pub mod deadline;
pub mod endpoint;
pub mod hex;
pub mod protocol;
pub mod secret;
pub mod vocabulary;
