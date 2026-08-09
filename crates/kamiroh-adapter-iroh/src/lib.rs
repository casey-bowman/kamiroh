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

use iroh::endpoint::presets::{Minimal, N0};
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

/// How far this node can be reached from.
///
/// The two options differ in what they *publish*, not only in what they can
/// connect to, which is why this is a deliberate choice rather than a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reach {
    /// Only at an address the caller already knows. Publishes nothing.
    ///
    /// No relays and no address lookup: a peer must be given `host:port`, and
    /// nothing about this node leaves the machine except to peers it talks to.
    /// Correct for a LAN, for tests, and for anyone who has not decided to be
    /// findable.
    #[default]
    Direct,

    /// From anywhere, by endpoint id alone.
    ///
    /// Number 0's relays and address lookup. This is what makes the
    /// home-to-cafe case work, and it has a price worth stating plainly:
    ///
    /// - **This node publishes its addresses.** A signed record — relay URL and
    ///   direct IPs — goes to n0's DNS/pkarr service under this node's endpoint
    ///   id. Anyone holding the id can then resolve where the node is, whether
    ///   or not the allowlist would admit them.
    /// - **A relay may carry the traffic.** When a direct path cannot be
    ///   established, packets pass through n0's relay. QUIC is end-to-end
    ///   encrypted, so a relay sees which endpoints are talking, when, and how
    ///   much — not what they say.
    ///
    /// The allowlist is unaffected: reachable is not the same as admitted. But
    /// "unlisted peers cannot find me" stops being true, and that is a real
    /// change to a node's exposure.
    Anywhere,
}

impl Reach {
    /// Whether a peer can be dialled from its endpoint id with no address.
    pub fn resolves_by_id(self) -> bool {
        matches!(self, Self::Anywhere)
    }

    /// A short description for a startup line.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Direct => "direct only — no relays, no discovery, nothing published",
            Self::Anywhere => "n0 relays + address lookup — this node's addresses are published",
        }
    }
}

/// Binds an Iroh endpoint for this node, listening for the kamiroh ALPN.
///
/// [`Reach::Direct`] uses the `Minimal` preset — the mandatory TLS crypto
/// provider and nothing else. [`Reach::Anywhere`] uses the `N0` preset, which
/// adds relays and address lookup; read that variant's documentation before
/// choosing it, because it publishes.
pub async fn bind_endpoint(
    secret: &NodeSecret,
    reach: Reach,
) -> Result<Endpoint, iroh::endpoint::BindError> {
    let key = SecretKey::from_bytes(secret.expose_bytes());
    let alpns = vec![ALPN.to_vec()];

    // The two presets are different types, so the builder cannot be shared.
    match reach {
        Reach::Direct => {
            Endpoint::builder(Minimal)
                .secret_key(key)
                .alpns(alpns)
                .bind()
                .await
        }
        Reach::Anywhere => {
            Endpoint::builder(N0)
                .secret_key(key)
                .alpns(alpns)
                .bind()
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(byte: u8) -> NodeSecret {
        NodeSecret::from_bytes([byte; 32])
    }

    #[test]
    fn only_anywhere_resolves_a_bare_endpoint_id() {
        // The property the transport branches on. `Direct` must never dial an
        // id it has no address for, or a misconfigured node would silently
        // start depending on a lookup service it was told not to use.
        assert!(!Reach::Direct.resolves_by_id());
        assert!(Reach::Anywhere.resolves_by_id());
    }

    #[test]
    fn the_default_reach_publishes_nothing() {
        // Opt-in is the whole point: `anywhere` announces this node's addresses
        // to a public service, and that must not happen by omission.
        assert_eq!(Reach::default(), Reach::Direct);
    }

    #[test]
    fn each_reach_says_what_it_does_to_a_startup_line() {
        assert!(Reach::Direct.describe().contains("nothing published"));
        assert!(Reach::Anywhere.describe().contains("published"));
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
