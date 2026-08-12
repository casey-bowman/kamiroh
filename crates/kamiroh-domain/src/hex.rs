//! Hex-string value objects for keys and identifiers.

use std::fmt;

/// A non-empty, lowercase hexadecimal string.
///
/// Used as the domain representation of key material identifiers (e.g. an
/// endpoint's public key). Construction validates; the inner string is
/// normalized to lowercase so equality and ordering behave.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hex(String);

impl Hex {
    /// Validate and normalize a hex string.
    pub fn new(s: impl Into<String>) -> Result<Self, HexError> {
        let s = s.into();
        if s.is_empty() {
            return Err(HexError::Empty);
        }
        if !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(HexError::NonHexCharacter);
        }
        Ok(Self(s.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexError {
    Empty,
    NonHexCharacter,
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexError::Empty => f.write_str("hex string is empty"),
            HexError::NonHexCharacter => f.write_str("hex string contains a non-hex character"),
        }
    }
}

impl std::error::Error for HexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_to_lowercase() {
        let h = Hex::new("DEADbeef01").unwrap();
        assert_eq!(h.as_str(), "deadbeef01");
    }

    #[test]
    fn rejects_empty_and_non_hex() {
        assert_eq!(Hex::new(""), Err(HexError::Empty));
        assert_eq!(Hex::new("xyz"), Err(HexError::NonHexCharacter));
    }
}
