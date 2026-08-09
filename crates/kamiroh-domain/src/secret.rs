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
}

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
}
