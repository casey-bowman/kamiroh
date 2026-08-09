//! In-memory [`Allowlist`].

use std::collections::HashSet;
use std::sync::RwLock;

use kamiroh_domain::EndpointId;
use kamiroh_ports::Allowlist;

/// An allowlist held in memory.
///
/// Deny-by-default: a newly created list permits nothing. There is intentionally
/// no constructor that permits everything.
#[derive(Debug, Default)]
pub struct InMemoryAllowlist {
    allowed: RwLock<HashSet<EndpointId>>,
}

impl InMemoryAllowlist {
    /// Creates an empty allowlist, which denies every endpoint.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an allowlist permitting exactly `endpoints`.
    pub fn with_endpoints(endpoints: impl IntoIterator<Item = EndpointId>) -> Self {
        Self {
            allowed: RwLock::new(endpoints.into_iter().collect()),
        }
    }

    /// Permits `endpoint`. Returns `true` if it was not already permitted.
    pub fn allow(&self, endpoint: EndpointId) -> bool {
        self.allowed
            .write()
            .expect("allowlist lock poisoned")
            .insert(endpoint)
    }

    /// Revokes `endpoint`. Returns `true` if it had been permitted.
    pub fn revoke(&self, endpoint: &EndpointId) -> bool {
        self.allowed
            .write()
            .expect("allowlist lock poisoned")
            .remove(endpoint)
    }

    /// How many endpoints are permitted.
    pub fn len(&self) -> usize {
        self.allowed.read().expect("allowlist lock poisoned").len()
    }

    /// Whether the list permits nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Allowlist for InMemoryAllowlist {
    fn is_allowed(&self, endpoint: &EndpointId) -> bool {
        self.allowed
            .read()
            .expect("allowlist lock poisoned")
            .contains(endpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(byte: u8) -> EndpointId {
        EndpointId::from_bytes([byte; 32])
    }

    #[test]
    fn empty_allowlist_denies_everything() {
        let allowlist = InMemoryAllowlist::new();
        assert!(allowlist.is_empty());
        for byte in [0, 1, 42, 128, 255] {
            assert!(
                !allowlist.is_allowed(&endpoint(byte)),
                "empty allowlist must deny endpoint {byte}"
            );
        }
    }

    #[test]
    fn allows_only_listed_endpoints() {
        let allowlist = InMemoryAllowlist::with_endpoints([endpoint(1), endpoint(2)]);
        assert!(allowlist.is_allowed(&endpoint(1)));
        assert!(allowlist.is_allowed(&endpoint(2)));
        assert!(!allowlist.is_allowed(&endpoint(3)));
    }

    #[test]
    fn allow_and_revoke_change_the_decision() {
        let allowlist = InMemoryAllowlist::new();
        assert!(!allowlist.is_allowed(&endpoint(9)));

        assert!(allowlist.allow(endpoint(9)));
        assert!(!allowlist.allow(endpoint(9)), "second allow is a no-op");
        assert!(allowlist.is_allowed(&endpoint(9)));

        assert!(allowlist.revoke(&endpoint(9)));
        assert!(!allowlist.is_allowed(&endpoint(9)));
        assert!(!allowlist.revoke(&endpoint(9)), "second revoke is a no-op");
    }

    #[test]
    fn revoking_the_last_endpoint_denies_everything_again() {
        let allowlist = InMemoryAllowlist::with_endpoints([endpoint(4)]);
        allowlist.revoke(&endpoint(4));
        assert!(allowlist.is_empty());
        assert!(!allowlist.is_allowed(&endpoint(4)));
    }
}
