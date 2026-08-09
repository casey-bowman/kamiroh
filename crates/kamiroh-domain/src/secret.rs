//! Node key material.
//!
//! [`NodeSecret`] is the private half of a node's identity. The domain models it
//! as opaque bytes so that key custody can be reviewed in one place, independent
//! of whichever transport eventually consumes it.

use core::fmt;

/// Length in bytes of a [`NodeSecret`].
pub const NODE_SECRET_LEN: usize = 32;

/// A node's private key material.
///
/// Three deliberate properties:
///
/// - [`fmt::Debug`] is redacted, so a secret cannot be logged by accident.
/// - The bytes are zeroed on drop.
/// - Reading the bytes requires the explicit [`NodeSecret::expose_bytes`] call,
///   which makes every use greppable during review.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeSecret([u8; NODE_SECRET_LEN]);

impl NodeSecret {
    /// Wraps raw key material.
    pub const fn from_bytes(bytes: [u8; NODE_SECRET_LEN]) -> Self {
        Self(bytes)
    }

    /// Borrows the raw key material.
    ///
    /// Named to be conspicuous: every call site is a place where key material
    /// leaves custody, and should be reviewed as such.
    pub const fn expose_bytes(&self) -> &[u8; NODE_SECRET_LEN] {
        &self.0
    }

    /// Builds a secret by filling it **in place**.
    ///
    /// Prefer this to [`NodeSecret::from_bytes`] whenever the material is
    /// freshly generated. `from_bytes` takes its argument by value, so the
    /// caller's array survives the call as an unprotected copy; here the bytes
    /// only ever exist inside the secret, and a failing `fill` drops a secret
    /// that zeroes whatever it had already written.
    ///
    /// The closure keeps the domain free of any RNG dependency: the adapter
    /// supplies the entropy source.
    pub fn from_fill<E>(
        fill: impl FnOnce(&mut [u8; NODE_SECRET_LEN]) -> Result<(), E>,
    ) -> Result<Self, E> {
        let mut secret = Self([0u8; NODE_SECRET_LEN]);
        fill(&mut secret.0)?;
        Ok(secret)
    }

    /// Parses lowercase-or-uppercase hex into a secret, in place.
    ///
    /// The error deliberately carries **no** detail about the input — not the
    /// offending character, not its position — because the input is key
    /// material and the error may be logged.
    pub fn from_hex(hex: &[u8]) -> Result<Self, ParseNodeSecretError> {
        Self::from_fill(|bytes| {
            crate::hex::decode_into(hex, bytes).map_err(|error| match error {
                crate::hex::HexError::Length { got } => ParseNodeSecretError::Length { got },
                crate::hex::HexError::NotHex { .. } => ParseNodeSecretError::NotHex,
            })
        })
    }

    /// Writes the secret as lowercase hex into `out`.
    ///
    /// Takes a caller-supplied buffer rather than returning a `String` so the
    /// caller decides where the encoded copy lives and can wipe it afterwards.
    pub fn write_hex_into(&self, out: &mut [u8; NODE_SECRET_LEN * 2]) {
        crate::hex::encode_into(&self.0, out);
    }
}

/// Why some bytes could not be read as a [`NodeSecret`].
///
/// Carries no fragment of the input: these values describe key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseNodeSecretError {
    /// The input was not exactly `NODE_SECRET_LEN * 2` hex characters.
    Length {
        /// The input length supplied. Not key material.
        got: usize,
    },
    /// The input contained a character outside `[0-9a-fA-F]`.
    NotHex,
}

impl fmt::Display for ParseNodeSecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { got } => write!(
                f,
                "node secret must be {} hex characters, got {got}",
                NODE_SECRET_LEN * 2
            ),
            Self::NotHex => f.write_str("node secret contains a non-hex character"),
        }
    }
}

impl core::error::Error for ParseNodeSecretError {}

/// Redacted: never prints key material.
impl fmt::Debug for NodeSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NodeSecret(<redacted>)")
    }
}

impl Drop for NodeSecret {
    fn drop(&mut self) {
        // Volatile writes plus a fence so the compiler cannot elide the wipe as
        // a dead store. This is the no-dependency equivalent of `zeroize`; if
        // the domain ever gains dependencies, swap it for that crate.
        for byte in &mut self.0 {
            unsafe { core::ptr::write_volatile(byte, 0) };
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_key_material() {
        let secret = NodeSecret::from_bytes([0xab; NODE_SECRET_LEN]);
        let rendered = format!("{secret:?}");
        assert_eq!(rendered, "NodeSecret(<redacted>)");
        assert!(!rendered.contains("ab"));
        assert!(!rendered.contains("171"));
    }

    #[test]
    fn exposes_the_bytes_it_was_given() {
        let secret = NodeSecret::from_bytes([7u8; NODE_SECRET_LEN]);
        assert_eq!(secret.expose_bytes(), &[7u8; NODE_SECRET_LEN]);
    }

    #[test]
    fn from_fill_writes_in_place() {
        let secret = NodeSecret::from_fill::<core::convert::Infallible>(|bytes| {
            bytes.fill(0x5a);
            Ok(())
        })
        .unwrap();
        assert_eq!(secret.expose_bytes(), &[0x5a; NODE_SECRET_LEN]);
    }

    #[test]
    fn from_fill_propagates_failure() {
        let result = NodeSecret::from_fill::<&str>(|_| Err("entropy source failed"));
        assert_eq!(result.unwrap_err(), "entropy source failed");
    }

    #[test]
    fn hex_round_trips() {
        let secret = NodeSecret::from_bytes([0xa7; NODE_SECRET_LEN]);
        let mut hex = [0u8; NODE_SECRET_LEN * 2];
        secret.write_hex_into(&mut hex);
        assert_eq!(hex, b"a7".repeat(NODE_SECRET_LEN).as_slice());

        let parsed = NodeSecret::from_hex(&hex).unwrap();
        assert_eq!(parsed.expose_bytes(), secret.expose_bytes());
    }

    #[test]
    fn hex_parsing_accepts_uppercase() {
        let hex = b"AB".repeat(NODE_SECRET_LEN);
        let parsed = NodeSecret::from_hex(&hex).unwrap();
        assert_eq!(parsed.expose_bytes(), &[0xab; NODE_SECRET_LEN]);
    }

    #[test]
    fn hex_parsing_rejects_wrong_length() {
        assert_eq!(
            NodeSecret::from_hex(b"abcd"),
            Err(ParseNodeSecretError::Length { got: 4 })
        );
    }

    #[test]
    fn hex_parse_errors_never_echo_the_input() {
        // The input is key material, so no error may quote any part of it.
        let hex = b"ff"
            .repeat(NODE_SECRET_LEN - 1)
            .into_iter()
            .chain(*b"zz")
            .collect::<Vec<_>>();
        let error = NodeSecret::from_hex(&hex).unwrap_err();
        assert_eq!(error, ParseNodeSecretError::NotHex);

        let rendered = format!("{error}");
        assert!(!rendered.contains("ff"), "{rendered}");
        assert!(!rendered.contains('z'), "{rendered}");
    }
}
