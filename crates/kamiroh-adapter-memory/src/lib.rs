//! In-memory kamiroh adapters.
//!
//! One implementation of every driven port, so the binary can be wired and run
//! end to end before Iroh, Kameo, or Herdr exist. These are development and test
//! doubles: none of them persists anything, and none is safe on a node that real
//! peers can reach.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod allowlist;
pub mod controller;
pub mod key_store;
pub mod transport;

pub use allowlist::InMemoryAllowlist;
pub use controller::EchoController;
pub use key_store::InMemoryKeyStore;
pub use transport::LoopbackTransport;

// `placeholder_endpoint_for` lived here until slice F1. It inverted the secret's
// bytes to fake an endpoint id, and it is deleted rather than left unused: a
// fake key derivation sitting in a test-double crate is available to be called
// by mistake. The real derivation is `kamiroh_adapter_iroh::endpoint_id_for`.
