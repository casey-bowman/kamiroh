//! Filesystem-backed kamiroh adapters.
//!
//! Today this is key custody: [`FileKeyStore`] gives a node a real, persistent
//! identity, generated from OS entropy and stored where only its owner can read
//! it. It replaces `kamiroh-adapter-memory`'s development key store, which held
//! a fixed, publicly known secret in memory.
//!
//! This crate deals in key *custody* only. Deriving a node's public
//! [`EndpointId`](kamiroh_domain::EndpointId) from its secret is ed25519 work
//! that belongs to the Iroh adapter.

// `deny` rather than `forbid`: the wipe buffers in `wipe` need `write_volatile`
// and carry a localised `allow`. Nothing else in the crate may use `unsafe`.
#![deny(unsafe_code)]
#![deny(missing_docs)]

pub mod key_store;

mod wipe;

pub use key_store::FileKeyStore;
