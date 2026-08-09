//! Agent controller (driven port).
//!
//! One long-lived controller actor drives one agent. This port is the *message
//! path* to that actor, not the actor itself: `kamiroh-adapter-kameo` will
//! implement it on top of a Kameo actor, and the application layer never sees
//! `kameo::Actor`.
//!
//! kamiroh is agent-agnostic. The controller relays [`ControlMessage`]s; what an
//! agent does with a prompt payload is entirely outside kamiroh.

use async_trait::async_trait;
use kamiroh_domain::{ActorName, ControlMessage, ControlReply};

/// Routes control messages to the controller actor for a named agent.
#[async_trait]
pub trait AgentController: Send + Sync + 'static {
    /// Delivers `message` to `agent`'s controller and awaits its reply.
    async fn dispatch(
        &self,
        agent: &ActorName,
        message: ControlMessage,
    ) -> Result<ControlReply, ControllerError>;
}

/// Why a control message did not reach a controller, or produced no reply.
#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    /// No controller actor is registered under that name on this node.
    #[error("no controller actor named {actor}")]
    NoSuchActor {
        /// The name that did not resolve.
        actor: String,
    },

    /// The controller actor exists but has stopped.
    #[error("controller actor {actor} has stopped")]
    Stopped {
        /// The name of the stopped actor.
        actor: String,
    },

    /// The controller cannot accept this message in its current state.
    #[error("controller actor {actor} rejected the message: {reason}")]
    Rejected {
        /// The name of the actor that refused.
        actor: String,
        /// Why it refused.
        reason: String,
    },

    /// The controller did not reply in time.
    #[error("timed out waiting for controller actor {actor}")]
    Timeout {
        /// The name of the actor that did not reply.
        actor: String,
    },

    /// The controller runtime failed for some other reason.
    #[error("controller backend failed: {0}")]
    Backend(#[source] Box<dyn core::error::Error + Send + Sync>),
}
