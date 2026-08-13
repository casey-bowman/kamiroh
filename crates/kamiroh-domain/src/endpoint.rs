//! Endpoint identity.

use std::fmt;

use crate::hex::Hex;

/// The transport-proven identity of an Iroh endpoint: its public key,
/// hex-encoded.
///
/// This is the unit of trust in kamiroh — the transport can prove which
/// `EndpointId` a delivery came from, and the allowlist admits `EndpointId`s
/// only. The domain stores it as a [`Hex`] value; the transport adapter owns
/// the conversion to and from concrete key types.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointId(Hex);

impl EndpointId {
    pub fn new(hex: Hex) -> Self {
        Self(hex)
    }

    pub fn as_hex(&self) -> &Hex {
        &self.0
    }
}

impl fmt::Display for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
