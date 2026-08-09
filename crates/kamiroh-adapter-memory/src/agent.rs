//! In-memory [`Agent`] that echoes its prompt.
//!
//! Lived in `kamiroh-adapter-kameo` from slice G, and moved here in M1 when
//! `Agent` became a port: an in-memory implementation of a driven port is what
//! this crate is for.
//!
//! It is the stand-in for a real agent runtime, and the last one left in
//! kamiroh. Everything beneath it — identity, allowlist, transport, front,
//! controller actors — is real; `kamiroh-adapter-herdr` supplies an agent that
//! is too. This remains useful for tests, and for running a node where no
//! agent runtime is available.

use async_trait::async_trait;
use kamiroh_domain::Payload;
use kamiroh_ports::{Agent, AgentError, AgentOutcome};

/// An agent that returns its prompt, finished.
#[derive(Debug, Clone, Copy, Default)]
pub struct EchoAgent;

#[async_trait]
impl Agent for EchoAgent {
    async fn run(&self, prompt: Payload) -> Result<AgentOutcome, AgentError> {
        // Always finished, and never failing: an echo has nothing to wait for,
        // nothing left to do, and no runtime to lose. Tests needing `Blocked`,
        // `Busy` or a failure should say so explicitly rather than hoping for
        // them here.
        Ok(AgentOutcome::finished(prompt))
    }
}

#[cfg(test)]
mod tests {
    use kamiroh_domain::AgentStatus;

    use super::*;

    #[tokio::test]
    async fn echo_returns_the_prompt_finished() {
        let prompt = Payload::text("ping");
        let outcome = EchoAgent.run(prompt.clone()).await.unwrap();

        assert_eq!(outcome.output, prompt);
        assert_eq!(outcome.status, AgentStatus::Idle);
        assert!(outcome.is_finished());
    }
}
