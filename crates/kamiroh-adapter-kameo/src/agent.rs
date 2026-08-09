//! What a controller actor drives.
//!
//! kamiroh is agent-agnostic: it routes and authorises control messages and
//! never interprets what an agent does with a prompt. [`Agent`] is the seam
//! where a real agent runtime plugs in — a subprocess, a model client, a shell
//! session — without any of that reaching the application layer.
//!
//! This trait deliberately lives in the adapter rather than in
//! `kamiroh-ports`. The ports crate describes kamiroh's boundaries; this
//! describes how *this* adapter runs the thing behind one of them. Promoting it
//! to a port would make every future controller adapter implement the same
//! notion of "an agent", which is exactly the assumption kamiroh avoids.

use async_trait::async_trait;
use kamiroh_domain::Payload;

/// The work behind one agent.
///
/// `run` takes a prompt and eventually produces output. It borrows `&self`
/// rather than `&mut self` because the controller actor runs it as a separate
/// task, so that the actor's mailbox stays live and an
/// [`Interrupt`](kamiroh_domain::ControlMessage::Interrupt) can land while the
/// agent is still working. An implementation needing mutable state should hold
/// it behind its own lock.
///
/// Cancellation is by drop: an interrupted or shut-down agent has its task
/// aborted, so `run` must leave no state that a dropped future would corrupt.
#[async_trait]
pub trait Agent: Send + Sync + 'static {
    /// Runs one prompt to completion.
    async fn run(&self, prompt: Payload) -> Payload;
}

/// An agent that returns its prompt unchanged.
///
/// The stand-in until a real agent runtime lands. It is not a placeholder in
/// the way `EchoController` was — that one faked the *controller*, so an
/// agent's lifecycle was simulated by a `HashMap`. Here the controller, its
/// mailbox and its lifecycle are all real; only the work is trivial.
#[derive(Debug, Clone, Copy, Default)]
pub struct EchoAgent;

#[async_trait]
impl Agent for EchoAgent {
    async fn run(&self, prompt: Payload) -> Payload {
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_returns_the_prompt_unchanged() {
        let prompt = Payload::text("ping");
        assert_eq!(EchoAgent.run(prompt.clone()).await, prompt);
    }
}
