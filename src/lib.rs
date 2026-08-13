//! kamiroh — Kameo actors for agents over Iroh.
//!
//! Peer actors, addressable by name and endpoint, that message each other —
//! locally or across the network — to drive agents.
//!
//! This facade crate is what embedding applications depend on. It re-exports
//! the workspace's crates under stable module names; see `ARCHITECTURE.md`
//! for the hexagon they form.

pub use kamiroh_adapter_iroh as adapter_iroh;
pub use kamiroh_adapter_kameo as adapter_kameo;
pub use kamiroh_adapter_memory as adapter_memory;
pub use kamiroh_app as app;
pub use kamiroh_domain as domain;
pub use kamiroh_ports as ports;
