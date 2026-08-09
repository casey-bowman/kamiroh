//! Iroh adapter for kamiroh.
//!
//! Two halves of one protocol:
//!
//! - [`IrohTransport`] implements the driven `Transport` port — this node
//!   sending control messages to actors on other nodes.
//! - [`front::serve`] is the driving side — inbound connections calling
//!   `ControlApi` with the peer Iroh authenticated.
//!
//! [`endpoint_id_for`] ties them to key custody: a node's public identity is the
//! ed25519 public key of the secret `kamiroh-adapter-fs` persists.
//!
//! # The conversion boundary
//!
//! This crate is the one place where an Iroh type meets a kamiroh type. Iroh's
//! `EndpointId` is an ed25519 public key; kamiroh's is an opaque 32-byte value
//! that the domain owns. They have the same bytes and deliberately different
//! types — the domain must not learn what an `iroh::PublicKey` is, or the
//! dependency rule is broken and swapping the transport later means rewriting
//! the domain. Everything above this crate speaks
//! [`kamiroh_domain::EndpointId`].

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod codec;
pub mod front;
pub mod transport;

pub use iroh::EndpointAddr;
pub use transport::{IrohTransport, peer_address};

use iroh::endpoint::presets::Minimal;
use iroh::{Endpoint, SecretKey};
use kamiroh_domain::{EndpointId, NodeSecret};

/// ALPN identifying the kamiroh control protocol.
///
/// Versioned in the name: a future incompatible protocol takes a new ALPN, so
/// mismatched peers fail to negotiate rather than misinterpreting each other.
pub const ALPN: &[u8] = b"kamiroh/control/0";

/// Derives this node's public endpoint id from its secret.
///
/// The id is the ed25519 public key for `secret`, which is exactly what Iroh
/// uses to identify and authenticate a node — so an id derived here is the id a
/// peer will see, without kamiroh reimplementing the derivation.
///
/// Deterministic: the same secret always yields the same id, which is what makes
/// a node's identity stable across restarts now that the key store persists the
/// secret.
pub fn endpoint_id_for(secret: &NodeSecret) -> EndpointId {
    // Infallible: any 32 bytes are a valid ed25519 secret scalar.
    let key = SecretKey::from_bytes(secret.expose_bytes());
    EndpointId::from_bytes(*key.public().as_bytes())
}

/// Binds an Iroh endpoint for this node, listening for the kamiroh ALPN.
///
/// Uses the [`Minimal`] preset: it sets the mandatory TLS crypto provider and
/// nothing else — no relays, no discovery. kamiroh addresses peers explicitly
/// (see [`IrohTransport`]), and relays are a NAT aid rather than a control path,
/// so bringing them in is a later, additive decision.
pub async fn bind_endpoint(secret: &NodeSecret) -> Result<Endpoint, iroh::endpoint::BindError> {
    Endpoint::builder(Minimal)
        .secret_key(SecretKey::from_bytes(secret.expose_bytes()))
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(byte: u8) -> NodeSecret {
        NodeSecret::from_bytes([byte; 32])
    }

    #[test]
    fn derivation_is_deterministic() {
        assert_eq!(endpoint_id_for(&secret(7)), endpoint_id_for(&secret(7)));
    }

    #[test]
    fn different_secrets_give_different_ids() {
        assert_ne!(endpoint_id_for(&secret(1)), endpoint_id_for(&secret(2)));
    }

    #[test]
    fn the_id_is_a_real_derivation_not_the_old_placeholder() {
        // Slice E shipped with `placeholder_endpoint_for`, which inverted the
        // secret's bytes. This pins that the fake is gone rather than reproduced:
        // a real public key is neither the secret nor any trivial transform of it.
        let secret = secret(0x0f);
        let id = endpoint_id_for(&secret);

        assert_ne!(id.as_bytes(), secret.expose_bytes(), "id equals the secret");

        let inverted = secret.expose_bytes().map(|byte| !byte);
        assert_ne!(id.as_bytes(), &inverted, "id is the old byte inversion");
    }

    #[test]
    fn a_derived_id_round_trips_through_its_wire_form() {
        // Peers exchange ids as hex, so a real derived id must survive the trip.
        let id = endpoint_id_for(&secret(0xa3));
        let parsed: EndpointId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn derivation_agrees_with_iroh_itself() {
        // The property the whole adapter exists for: what we hand upward is the
        // same id Iroh would present to a peer.
        let secret = secret(0x5c);
        let iroh_id = SecretKey::from_bytes(secret.expose_bytes()).public();
        assert_eq!(endpoint_id_for(&secret).as_bytes(), iroh_id.as_bytes());
    }
}
