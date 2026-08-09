//! The work behind an agent (driven port).
//!
//! [`AgentController`](crate::AgentController) is the message path to an
//! agent's controller. This is the thing on the far side of it: what actually
//! happens when a prompt arrives.
//!
//! # This was an adapter trait until M1
//!
//! `Agent` lived in `kamiroh-adapter-kameo` from slice G, and the note there
//! argued it should stay: *"the ports crate describes kamiroh's boundaries;
//! this describes how that adapter runs the thing behind one. Promoting it
//! would make every future controller adapter adopt one notion of an agent."*
//!
//! That held while one crate both defined and implemented it. It stopped
//! holding when a second adapter arrived to implement it —
//! `kamiroh-adapter-herdr`, whose agents are the ones Herdr manages. A trait
//! that one adapter drives and another satisfies is a boundary by definition,
//! and the alternative was an adapter depending on an adapter.
//!
//! The agnosticism concern was right to raise and survives the move: nothing
//! here says what an agent *does*. A prompt goes in, output and a state come
//! back, and both are opaque.
//!
//! # Cancellation
//!
//! Cancellation is by drop: an interrupted or shut-down agent has its task
//! aborted, so [`run`](Agent::run) must leave no state that a dropped future
//! would corrupt.

use async_trait::async_trait;
use kamiroh_domain::{AgentStatus, Payload};

/// The work behind one agent.
///
/// `run` borrows `&self` rather than `&mut self` because a controller runs it
/// as a separate task, so the controller's mailbox stays live and an
/// [`Interrupt`](kamiroh_domain::ControlMessage::Interrupt) can land while the
/// agent is still working. An implementation needing mutable state should hold
/// it behind its own lock.
#[async_trait]
pub trait Agent: Send + Sync + 'static {
    /// Runs one prompt for as long as the implementation is willing to wait.
    ///
    /// Returning is **not** a claim that the agent is finished — see
    /// [`AgentOutcome::status`]. An agent that stops to ask a question, or one
    /// that is simply slower than the caller's patience, returns what it has
    /// and says so.
    ///
    /// Failing is different from producing nothing. An agent runtime that
    /// cannot be reached must be an [`AgentError`], never an empty
    /// [`AgentOutcome`]: the alternative is an infrastructure failure arriving
    /// at the caller looking like something the agent said.
    async fn run(&self, prompt: Payload) -> Result<AgentOutcome, AgentError>;
}

/// Why a run produced no outcome at all.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// The agent runtime could not be reached.
    #[error("the agent runtime is unavailable: {detail}")]
    Unavailable {
        /// What went wrong reaching it.
        detail: String,
    },

    /// The runtime is there, but cannot accept this prompt.
    #[error("the agent cannot take this prompt: {reason}")]
    Unsupported {
        /// Why not.
        reason: String,
    },

    /// The runtime failed for some other reason.
    #[error("agent backend failed: {0}")]
    Backend(#[source] Box<dyn core::error::Error + Send + Sync>),
}

/// So a composition root can pick an implementation at runtime.
///
/// Choosing between an agent runtime and a stand-in is a decision made from
/// configuration, which means the two arms have different concrete types and
/// have to meet as `Arc<dyn Agent>` before anything else can take them.
#[async_trait]
impl Agent for std::sync::Arc<dyn Agent> {
    async fn run(&self, prompt: Payload) -> Result<AgentOutcome, AgentError> {
        (**self).run(prompt).await
    }
}

/// What one run produced, and where the agent left off.
///
/// The status is the half that is easy to leave out and expensive to add back.
/// Without it a caller on another node cannot tell "finished" from "waiting for
/// you", because the reply is the only thing that crosses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOutcome {
    /// What the agent produced during this run.
    pub output: Payload,
    /// Where the agent is now.
    pub status: AgentStatus,
}

impl AgentOutcome {
    /// The agent finished and is ready for more work.
    pub fn finished(output: Payload) -> Self {
        Self {
            output,
            status: AgentStatus::Idle,
        }
    }

    /// The agent stopped to ask a human something; `output` holds the question.
    pub fn blocked(output: Payload) -> Self {
        Self {
            output,
            status: AgentStatus::Blocked,
        }
    }

    /// The agent is still working; `output` is what there is so far.
    pub fn still_working(output: Payload) -> Self {
        Self {
            output,
            status: AgentStatus::Busy,
        }
    }

    /// Whether this run left the agent with nothing outstanding.
    pub fn is_finished(&self) -> bool {
        self.status == AgentStatus::Idle
    }
}
