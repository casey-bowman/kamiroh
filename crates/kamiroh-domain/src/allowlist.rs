//! Per-actor inbound policy.

use std::collections::BTreeSet;

use crate::endpoint::EndpointId;

/// The set of endpoints an actor will receive messages from.
///
/// Semantics (see `ARCHITECTURE.md`, decisions 2 and 3):
///
/// - Holds **endpoints only**. Admitting an endpoint means trusting that
///   endpoint's runtime, including its honesty about which of its actors is
///   speaking — names are claims, so a name-keyed policy would promise what
///   the transport cannot prove.
/// - **Deny by default**: an empty allowlist receives nothing.
/// - Intended to be **checked on every delivery**, not only at
///   conversation-open, so revocation takes effect on live connections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowlist {
    endpoints: BTreeSet<EndpointId>,
}

impl Allowlist {
    /// The deny-everything policy.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn admit(&mut self, endpoint: EndpointId) {
        self.endpoints.insert(endpoint);
    }

    pub fn revoke(&mut self, endpoint: &EndpointId) {
        self.endpoints.remove(endpoint);
    }

    pub fn allows(&self, endpoint: &EndpointId) -> bool {
        self.endpoints.contains(endpoint)
    }

    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Hex;

    fn endpoint(s: &str) -> EndpointId {
        EndpointId::new(Hex::new(s).unwrap())
    }

    #[test]
    fn empty_denies_everything() {
        let list = Allowlist::empty();
        assert!(!list.allows(&endpoint("aa")));
    }

    #[test]
    fn admit_then_allow() {
        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        assert!(list.allows(&endpoint("aa")));
        assert!(!list.allows(&endpoint("bb")));
    }

    #[test]
    fn revoke_takes_effect() {
        let mut list = Allowlist::empty();
        list.admit(endpoint("aa"));
        list.revoke(&endpoint("aa"));
        assert!(!list.allows(&endpoint("aa")));
    }
}
