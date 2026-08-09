//! Outbound peer transport (driven port).
//!
//! The port speaks in domain terms — [`PeerAddress`], [`ControlMessage`],
//! [`ControlReply`] — and says nothing about connections, streams, or wire
//! encoding. An adapter (Iroh, in-memory, ...) owns all of that.
//!
//! Inbound traffic is *not* modelled here: a transport adapter receives a
//! message and calls the driving port [`crate::control_api::ControlApi`].
//! Keeping the two directions separate is what lets several fronts (Iroh, Herdr)
//! reach the same controller actor.

use async_trait::async_trait;
use kamiroh_domain::{ControlMessage, ControlReply, EndpointId, PeerAddress};

/// Sends control messages to actors on remote nodes.
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// This node's own endpoint id, as other peers must address it.
    fn local_endpoint_id(&self) -> EndpointId;

    /// Sends `message` to `to` and waits for the controller's reply.
    async fn send(
        &self,
        to: &PeerAddress,
        message: ControlMessage,
    ) -> Result<ControlReply, TransportError>;
}

/// Why an outbound control message did not produce a reply.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The remote node could not be reached.
    #[error("peer {endpoint} is unreachable: {detail}")]
    Unreachable {
        /// The node that could not be reached.
        endpoint: EndpointId,
        /// Adapter-supplied detail.
        detail: String,
    },

    /// The remote node refused us — typically its allowlist does not include us.
    #[error("peer {endpoint} refused the connection")]
    Refused {
        /// The node that refused.
        endpoint: EndpointId,
    },

    /// The remote node has no actor by that name.
    #[error("no actor named {actor} on peer {endpoint}")]
    NoSuchActor {
        /// The node that was reached.
        endpoint: EndpointId,
        /// The actor name that did not resolve.
        actor: String,
    },

    /// The peer did not reply in time.
    #[error("timed out waiting for a reply from {endpoint}")]
    Timeout {
        /// The node that did not reply.
        endpoint: EndpointId,
    },

    /// A message could not be encoded or a reply could not be decoded.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// The underlying transport failed for some other reason.
    #[error("transport backend failed: {0}")]
    Backend(#[source] Box<dyn core::error::Error + Send + Sync>),
}
