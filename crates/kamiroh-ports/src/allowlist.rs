//! Endpoint allowlist (driven port).
//!
//! # Security contract
//!
//! This is the trust boundary for inbound connections. Two rules hold for every
//! implementation:
//!
//! 1. **Deny by default.** An empty allowlist rejects every endpoint. There is
//!    no "empty means allow all" mode, and no way to configure one.
//! 2. **No enumeration.** The trait deliberately offers no method returning the
//!    set of allowed endpoints. Callers ask about one endpoint at a time, so a
//!    caller cannot fetch the list and apply its own — possibly wrong — filter.
//!
//! The decision is a plain `bool` from a synchronous call: an allowlist check is
//! set membership, and making it fallible or async would invite callers to treat
//! an error as "allow".

use kamiroh_domain::EndpointId;

/// Decides which remote endpoints may talk to this node.
pub trait Allowlist: Send + Sync + 'static {
    /// Returns `true` only if `endpoint` is explicitly permitted.
    ///
    /// Implementations must return `false` for anything not explicitly allowed,
    /// including when their backing configuration is empty.
    fn is_allowed(&self, endpoint: &EndpointId) -> bool;
}
