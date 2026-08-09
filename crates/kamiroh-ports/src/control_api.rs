//! The control API (driving port).
//!
//! This is the port that *fronts* call: an Iroh adapter that has just received a
//! message from a peer, or a Herdr adapter relaying local pane input. Both reach
//! the same controller actor through this one trait, which is what "same actor,
//! multiple fronts" means in code.
//!
//! The application layer implements it. Adapters depend on it; it depends on no
//! adapter.

use async_trait::async_trait;
use kamiroh_domain::{ActorName, ControlMessage, ControlReply, EndpointId};

/// Where a control message came from.
///
/// This distinction carries the trust decision, so it is a type rather than a
/// convention. Local origin **bypasses the allowlist**, so the type is opaque
/// and its two cases are reachable only through named constructors:
///
/// - [`Origin::remote`] — checked against the allowlist.
/// - [`Origin::local_front`] — inside the trust boundary, allowlist skipped.
///
/// Making local trust a function call rather than an ambient enum variant means
/// `grep -r 'local_front'` lists every place in the tree that claims it, the
/// same audit affordance as [`kamiroh_domain::NodeSecret::expose_bytes`]. A
/// remote transport adapter that reaches for `local_front()` is visibly wrong;
/// a stray `Origin::Local` would not have been.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Origin(Provenance);

/// The private inner form of [`Origin`]. Not public: see the type's docs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Provenance {
    Remote(EndpointId),
    Local,
}

impl Origin {
    /// A message from a peer node, subject to the allowlist.
    ///
    /// `endpoint` must be the endpoint the transport **authenticated**, never one
    /// read out of message content.
    pub fn remote(endpoint: EndpointId) -> Self {
        Self(Provenance::Remote(endpoint))
    }

    /// A message from a front running on this node, such as a Herdr pane.
    ///
    /// **Skips the allowlist.** Only in-process fronts may call this; a remote
    /// transport adapter calling it would disable the trust boundary for every
    /// peer it serves.
    pub fn local_front() -> Self {
        Self(Provenance::Local)
    }

    /// The peer endpoint, or `None` when this message came from a local front.
    ///
    /// A `Some` answer is precisely the set of messages requiring authorisation.
    ///
    /// **This is the only reader.** An `is_local()` companion existed and had no
    /// callers; it was removed rather than kept, because a convenient predicate
    /// on the trust type is an invitation for an adapter to branch on trust
    /// somewhere other than `ControlService`, which §5 names as the one place
    /// authorisation happens.
    pub fn remote_endpoint(&self) -> Option<EndpointId> {
        match self.0 {
            Provenance::Remote(endpoint) => Some(endpoint),
            Provenance::Local => None,
        }
    }
}

/// Accepts control messages from any front and routes them to an agent.
#[async_trait]
pub trait ControlApi: Send + Sync + 'static {
    /// Authorises `origin`, then delivers `message` to `agent`'s controller.
    async fn deliver(
        &self,
        origin: Origin,
        agent: &ActorName,
        message: ControlMessage,
    ) -> Result<ControlReply, ControlApiError>;
}

/// Why a control message was not carried out.
#[derive(Debug, thiserror::Error)]
pub enum ControlApiError {
    /// The origin endpoint is not on this node's allowlist.
    ///
    /// Carries no detail beyond the endpoint: a rejected caller learns only that
    /// it was rejected, never anything about which agents exist here.
    #[error("endpoint {endpoint} is not allowed")]
    NotAllowed {
        /// The endpoint that was refused.
        endpoint: EndpointId,
    },

    /// The origin was allowed, but the controller could not be reached.
    #[error(transparent)]
    Controller(#[from] crate::controller::ControllerError),
}
