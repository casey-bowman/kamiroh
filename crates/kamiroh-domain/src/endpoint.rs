//! Endpoint identity.
//!
//! An [`EndpointId`] names a node on the network. It is deliberately an opaque
//! 32-byte value rather than a re-export of a transport library's key type: the
//! domain must not depend on Iroh (or any other transport). Adapters convert at
//! the boundary.

use core::fmt;
use core::str::FromStr;

/// Length in bytes of an [`EndpointId`].
pub const ENDPOINT_ID_LEN: usize = 32;

/// A node's public identity on the network.
///
/// Wire/display form is lowercase hex (64 characters). Parsing accepts either
/// case; [`fmt::Display`] always emits lowercase.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointId([u8; ENDPOINT_ID_LEN]);

impl EndpointId {
    /// Builds an endpoint id from its raw 32 bytes.
    pub const fn from_bytes(bytes: [u8; ENDPOINT_ID_LEN]) -> Self {
        Self(bytes)
    }

    /// Borrows the raw 32 bytes.
    pub const fn as_bytes(&self) -> &[u8; ENDPOINT_ID_LEN] {
        &self.0
    }
}

impl From<[u8; ENDPOINT_ID_LEN]> for EndpointId {
    fn from(bytes: [u8; ENDPOINT_ID_LEN]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for EndpointId {
    type Error = ParseEndpointIdError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let bytes: [u8; ENDPOINT_ID_LEN] = bytes
            .try_into()
            .map_err(|_| ParseEndpointIdError::Length { got: bytes.len() })?;
        Ok(Self(bytes))
    }
}

impl fmt::Display for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Shows the full hex id. Endpoint ids are public keys, so this is not secret.
impl fmt::Debug for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EndpointId({self})")
    }
}

impl FromStr for EndpointId {
    type Err = ParseEndpointIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != ENDPOINT_ID_LEN * 2 {
            return Err(ParseEndpointIdError::Length { got: s.len() });
        }
        let src = s.as_bytes();
        let mut out = [0u8; ENDPOINT_ID_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            let hi = hex_digit(src[i * 2])?;
            let lo = hex_digit(src[i * 2 + 1])?;
            *slot = (hi << 4) | lo;
        }
        Ok(Self(out))
    }
}

fn hex_digit(c: u8) -> Result<u8, ParseEndpointIdError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(ParseEndpointIdError::NotHex {
            found: char::from(c),
        }),
    }
}

/// Why a string could not be read as an [`EndpointId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseEndpointIdError {
    /// Input was not exactly 64 hex characters (or 32 bytes).
    Length {
        /// The length that was supplied.
        got: usize,
    },
    /// Input contained a character outside `[0-9a-fA-F]`.
    NotHex {
        /// The first non-hex character found.
        found: char,
    },
}

impl fmt::Display for ParseEndpointIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { got } => write!(
                f,
                "endpoint id must be {} hex characters, got {got}",
                ENDPOINT_ID_LEN * 2
            ),
            Self::NotHex { found } => write!(f, "endpoint id contains non-hex character {found:?}"),
        }
    }
}

impl core::error::Error for ParseEndpointIdError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EndpointId {
        let mut bytes = [0u8; ENDPOINT_ID_LEN];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        EndpointId::from_bytes(bytes)
    }

    #[test]
    fn display_then_parse_round_trips() {
        let id = sample();
        let parsed: EndpointId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn display_is_lowercase_hex_of_full_length() {
        let text = sample().to_string();
        assert_eq!(text.len(), ENDPOINT_ID_LEN * 2);
        assert!(text.starts_with("000102030405"));
        assert_eq!(text, text.to_lowercase());
    }

    #[test]
    fn parsing_accepts_uppercase() {
        let id = sample();
        let upper: EndpointId = id.to_string().to_uppercase().parse().unwrap();
        assert_eq!(id, upper);
    }

    #[test]
    fn parsing_rejects_wrong_length() {
        assert_eq!(
            "00ff".parse::<EndpointId>(),
            Err(ParseEndpointIdError::Length { got: 4 })
        );
    }

    #[test]
    fn parsing_rejects_non_hex() {
        let bad = "z".repeat(ENDPOINT_ID_LEN * 2);
        assert_eq!(
            bad.parse::<EndpointId>(),
            Err(ParseEndpointIdError::NotHex { found: 'z' })
        );
    }

    #[test]
    fn try_from_slice_requires_exact_length() {
        assert!(EndpointId::try_from([7u8; ENDPOINT_ID_LEN].as_slice()).is_ok());
        assert!(EndpointId::try_from([7u8; 31].as_slice()).is_err());
    }
}
