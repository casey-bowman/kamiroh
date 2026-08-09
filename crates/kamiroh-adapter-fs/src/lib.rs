//! Filesystem-backed kamiroh adapters.
//!
//! Two files under one config directory, holding the two halves of a node's
//! identity:
//!
//! - [`FileKeyStore`] — `node.key`, who this node *is*. Generated from OS
//!   entropy and stored where only its owner can read it. It replaces
//!   `kamiroh-adapter-memory`'s development key store, which held a fixed,
//!   publicly known secret in memory.
//! - [`FileAllowlist`] — `allow`, who this node will *talk to*. The inbound
//!   trust boundary, replacing the list the composition root used to assemble
//!   from an environment variable.
//!
//! Their custody rules differ, and the difference is the point. A node secret
//! is secret: unreadable by anyone else, or it is refused. An allowlist is
//! public keys: readable by anyone, but not *writable*, since an account that
//! can append a line can admit itself.
//!
//! This crate deals in storage only. Deriving a node's public
//! [`EndpointId`](kamiroh_domain::EndpointId) from its secret is ed25519 work
//! that belongs to the Iroh adapter.

// `deny` rather than `forbid`: the wipe buffers in `wipe` need `write_volatile`
// and carry a localised `allow`. Nothing else in the crate may use `unsafe`.
#![deny(unsafe_code)]
#![deny(missing_docs)]

pub mod allowlist;
pub mod key_store;

mod wipe;

pub use allowlist::{AllowlistError, FileAllowlist};
pub use key_store::FileKeyStore;
