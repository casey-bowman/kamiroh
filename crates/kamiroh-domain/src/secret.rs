//! Secret key material.

use std::fmt;

/// Secret key material backing an endpoint.
///
/// Deliberately opaque: `Debug` is redacted so a `Secret` cannot leak through
/// logs or error chains, and there is no `Display`. Code that legitimately
/// needs the bytes (the transport adapter constructing an Iroh endpoint) must
/// say so explicitly via [`Secret::expose`].
#[derive(Clone)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Explicit access to the raw bytes. Named to make call sites visible in
    /// review; do not call this outside adapter wiring.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let s = Secret::new(vec![1, 2, 3]);
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
    }
}
