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

use kamiroh_domain::{EndpointId, NodeSecret};

/// Derives a placeholder [`EndpointId`] from a [`NodeSecret`].
///
/// # Placeholder, not a key derivation
///
/// A real endpoint id is the ed25519 public key for the node secret. This copies
/// the secret's bytes and inverts them — enough to give the in-memory node a
/// stable, distinct-looking id, and **not** a key derivation of any kind. The
/// Iroh adapter replaces it with the real public key in slice F.
pub fn placeholder_endpoint_for(secret: &NodeSecret) -> EndpointId {
    let mut bytes = *secret.expose_bytes();
    for byte in &mut bytes {
        *byte = !*byte;
    }
    EndpointId::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_endpoint_is_stable_and_differs_from_the_secret() {
        let secret = NodeSecret::from_bytes([0x0f; 32]);
        let first = placeholder_endpoint_for(&secret);
        assert_eq!(first, placeholder_endpoint_for(&secret));
        assert_ne!(first.as_bytes(), secret.expose_bytes());
    }

    #[test]
    fn different_secrets_give_different_endpoints() {
        assert_ne!(
            placeholder_endpoint_for(&NodeSecret::from_bytes([1u8; 32])),
            placeholder_endpoint_for(&NodeSecret::from_bytes([2u8; 32]))
        );
    }
}
