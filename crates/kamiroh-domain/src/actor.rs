//! Actor naming and addressing.
//!
//! Locally an actor is reached by name; remotely by [`EndpointId`] + name.

use core::fmt;
use core::str::FromStr;

use crate::endpoint::EndpointId;

/// Maximum length of an [`ActorName`], in bytes.
pub const MAX_ACTOR_NAME_LEN: usize = 64;

/// The local name of a controller actor.
///
/// Names are restricted to ASCII alphanumerics plus `-`, `_` and `.`, so they
/// stay safe to use in wire framing, log lines, and file paths without escaping.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ActorName(String);

impl ActorName {
    /// Validates and wraps an actor name.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidActorName> {
        let name = name.into();
        if name.is_empty() {
            return Err(InvalidActorName::Empty);
        }
        if name.len() > MAX_ACTOR_NAME_LEN {
            return Err(InvalidActorName::TooLong { got: name.len() });
        }
        if let Some(found) = name.chars().find(|c| !is_allowed_char(*c)) {
            return Err(InvalidActorName::IllegalCharacter { found });
        }
        Ok(Self(name))
    }

    /// Borrows the name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_allowed_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

impl fmt::Display for ActorName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ActorName {
    type Err = InvalidActorName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl AsRef<str> for ActorName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Why a string was rejected as an [`ActorName`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidActorName {
    /// The name was the empty string.
    Empty,
    /// The name exceeded [`MAX_ACTOR_NAME_LEN`].
    TooLong {
        /// The rejected name's length in bytes.
        got: usize,
    },
    /// The name contained a character outside `[A-Za-z0-9._-]`.
    IllegalCharacter {
        /// The first illegal character found.
        found: char,
    },
}

impl fmt::Display for InvalidActorName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("actor name must not be empty"),
            Self::TooLong { got } => write!(
                f,
                "actor name must be at most {MAX_ACTOR_NAME_LEN} bytes, got {got}"
            ),
            Self::IllegalCharacter { found } => {
                write!(f, "actor name contains illegal character {found:?}")
            }
        }
    }
}

impl core::error::Error for InvalidActorName {}

/// A remote actor address: which node, and which actor on that node.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PeerAddress {
    /// The node hosting the actor.
    pub endpoint: EndpointId,
    /// The actor's local name on that node.
    pub actor: ActorName,
}

impl PeerAddress {
    /// Builds a peer address.
    pub fn new(endpoint: EndpointId, actor: ActorName) -> Self {
        Self { endpoint, actor }
    }
}

impl fmt::Display for PeerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.endpoint, self.actor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_names() {
        for name in ["agent", "coding-agent", "agent_1", "a.b-c_9"] {
            assert!(ActorName::new(name).is_ok(), "{name} should be valid");
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(ActorName::new(""), Err(InvalidActorName::Empty));
    }

    #[test]
    fn rejects_over_length() {
        let long = "a".repeat(MAX_ACTOR_NAME_LEN + 1);
        assert_eq!(
            ActorName::new(long),
            Err(InvalidActorName::TooLong {
                got: MAX_ACTOR_NAME_LEN + 1
            })
        );
    }

    #[test]
    fn rejects_path_and_whitespace_characters() {
        for (name, bad) in [("a/b", '/'), ("a b", ' '), ("a\nb", '\n'), ("../etc", '/')] {
            assert_eq!(
                ActorName::new(name),
                Err(InvalidActorName::IllegalCharacter { found: bad }),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn peer_address_displays_as_endpoint_slash_actor() {
        let address = PeerAddress::new(
            EndpointId::from_bytes([0xab; 32]),
            ActorName::new("agent").unwrap(),
        );
        assert_eq!(address.to_string(), format!("{}/agent", "ab".repeat(32)));
    }
}
