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
//! Cancellation is by drop: a given-up-on or detached agent has its task
//! aborted, so [`run`](Agent::run) must leave no state that a dropped future
//! would corrupt.

use async_trait::async_trait;
use kamiroh_domain::{AgentStatus, Payload};

/// The work behind one agent.
///
/// `run` borrows `&self` rather than `&mut self` because a controller runs it
/// as a separate task, so the controller's mailbox stays live and an
/// [`StopWaiting`](kamiroh_domain::ControlMessage::StopWaiting) can land while the
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

    /// What the agent is doing right now, if it can say.
    ///
    /// A controller's own view of an agent is only as fresh as the last run it
    /// finished, and **an agent can change state without kamiroh doing
    /// anything**. A coding agent that raises a permission dialog on startup is
    /// blocked before it has been prompted even once; without this, `Status`
    /// answers `Idle` and invites someone to wait for work that will never
    /// start.
    ///
    /// `Ok(None)` means "no better answer than yours" — the caller should keep
    /// what it had. That is the default, and it is right for any agent whose
    /// state only changes when it is run.
    async fn status(&self) -> Result<Option<AgentStatus>, AgentError> {
        Ok(None)
    }
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
/// **Every method must be forwarded here.** A defaulted trait method that this
/// impl does not override is silently answered by the *default* rather than by
/// the wrapped agent — which is not a compile error, and not visible until
/// something behaves as though the agent had no opinion. `status` was added
/// with a default and this impl kept it for one commit; the symptom was
/// kamiroh reporting `Idle` for an agent sitting at a permission dialog.
#[async_trait]
impl Agent for std::sync::Arc<dyn Agent> {
    async fn run(&self, prompt: Payload) -> Result<AgentOutcome, AgentError> {
        (**self).run(prompt).await
    }

    async fn status(&self) -> Result<Option<AgentStatus>, AgentError> {
        (**self).status().await
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// An agent with an opinion about its own state.
    struct Opinionated;

    #[async_trait]
    impl Agent for Opinionated {
        async fn run(&self, prompt: Payload) -> Result<AgentOutcome, AgentError> {
            Ok(AgentOutcome::blocked(prompt))
        }

        async fn status(&self) -> Result<Option<AgentStatus>, AgentError> {
            Ok(Some(AgentStatus::Blocked))
        }
    }

    /// The `Arc<dyn Agent>` forwarding impl must forward **every** method.
    ///
    /// A defaulted method it fails to override is answered by the default, not
    /// by the agent — silently, and with no compile error. That is how kamiroh
    /// came to report `Idle` for an agent stopped at a permission dialog.
    #[tokio::test]
    async fn the_arc_impl_forwards_status_and_not_the_default() {
        let direct = Opinionated;
        let boxed: Arc<dyn Agent> = Arc::new(Opinionated);

        assert_eq!(direct.status().await.unwrap(), Some(AgentStatus::Blocked));
        assert_eq!(
            boxed.status().await.unwrap(),
            Some(AgentStatus::Blocked),
            "Arc<dyn Agent> answered from the default instead of the agent"
        );
    }

    #[tokio::test]
    async fn the_arc_impl_forwards_run() {
        let boxed: Arc<dyn Agent> = Arc::new(Opinionated);
        let outcome = boxed.run(Payload::text("x")).await.unwrap();
        assert_eq!(outcome.status, AgentStatus::Blocked);
    }

    /// An agent whose state only changes when it runs keeps the default.
    #[tokio::test]
    async fn the_default_status_defers_to_the_caller() {
        struct Quiet;

        #[async_trait]
        impl Agent for Quiet {
            async fn run(&self, prompt: Payload) -> Result<AgentOutcome, AgentError> {
                Ok(AgentOutcome::finished(prompt))
            }
        }

        assert_eq!(Quiet.status().await.unwrap(), None);
    }
}
