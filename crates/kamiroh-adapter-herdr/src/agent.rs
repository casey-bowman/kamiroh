//! [`HerdrAgent`] — an agent that Herdr is managing.
//!
//! The first real [`Agent`] in kamiroh. A prompt goes to a coding agent running
//! in a Herdr pane, and what it says comes back.
//!
//! ```text
//! Agent::run(prompt)
//!   -> agent.prompt {target, text, wait: {until: [done, blocked], timeout_ms}}
//!   -> agent.read   {target, source: "recent", lines}
//!   -> AgentOutcome {output, status}
//! ```
//!
//! kamiroh does not start, supervise or parse the agent. Herdr does the first
//! two and nobody does the third — the output is an opaque
//! [`Payload`](kamiroh_domain::Payload) the whole way out to the peer.
//!
//! # Patience is set by the most impatient caller
//!
//! `agent.prompt` can wait as long as it likes. kamiroh cannot: the Iroh front
//! gives a request 30 seconds and the transport gives a reply 30 seconds, so a
//! remote prompt that waits longer is answered by a timeout instead of by the
//! agent. [`DEFAULT_PATIENCE`] is therefore well under that, and running out of
//! it is **not** a failure — it produces
//! [`AgentStatus::Busy`](kamiroh_domain::AgentStatus::Busy) and whatever the
//! agent had said by then.
//!
//! That is the honest shape for now, and it has a real limit: there is no way
//! to ask for the *rest* of a long answer, because `ControlMessage` has no verb
//! for it. A caller can prompt again and read more, which is a workaround
//! rather than a design. Adding that verb is a decision for a later slice, and
//! wants a real long-running agent to inform it.
//!
//! # What counts as output
//!
//! `agent.read` returns the last `lines` of the pane, ANSI stripped. A terminal
//! has no marker for "this is the answer to that prompt", so this is a
//! heuristic and openly one: it may include the prompt's own echo, or earlier
//! output if the agent said little.

use std::time::Duration;

use async_trait::async_trait;
use kamiroh_domain::Payload;
use kamiroh_ports::{Agent, AgentError, AgentOutcome};

use crate::client::{Client, ClientError};
use crate::pane::PaneAgentState;

/// How long to wait for an agent before answering with what it has so far.
///
/// Under the Iroh front's 30s request timeout with room to spare, so a remote
/// caller hears from the agent rather than from a timeout.
pub const DEFAULT_PATIENCE: Duration = Duration::from_secs(20);

/// How many lines of pane output a reply carries.
pub const DEFAULT_LINES: u32 = 200;

/// An agent that Herdr is managing, driven over Herdr's socket API.
#[derive(Debug)]
pub struct HerdrAgent {
    client: Client,
    target: String,
    patience: Duration,
    lines: u32,
}

impl HerdrAgent {
    /// Drives the Herdr agent named by `target` — a pane id or an agent name.
    pub fn new(client: Client, target: impl Into<String>) -> Self {
        Self {
            client,
            target: target.into(),
            patience: DEFAULT_PATIENCE,
            lines: DEFAULT_LINES,
        }
    }

    /// Overrides how long a run waits before returning what it has.
    ///
    /// Keep it below the front's request timeout, or remote callers will be
    /// answered by a timeout rather than by this agent.
    #[must_use]
    pub fn with_patience(mut self, patience: Duration) -> Self {
        self.patience = patience;
        self
    }

    /// Overrides how many lines of output a reply carries.
    #[must_use]
    pub fn with_lines(mut self, lines: u32) -> Self {
        self.lines = lines;
        self
    }

    /// What this agent is bound to.
    pub fn target(&self) -> &str {
        &self.target
    }
}

#[async_trait]
impl Agent for HerdrAgent {
    async fn run(&self, prompt: Payload) -> Result<AgentOutcome, AgentError> {
        // A pane takes keystrokes. Refusing non-text is better than sending
        // arbitrary bytes to a terminal and calling the result an answer.
        let text = prompt.as_text().ok_or_else(|| AgentError::Unsupported {
            reason: format!(
                "a Herdr pane takes text; this payload is {}",
                prompt.content_type()
            ),
        })?;

        let settled = self
            .client
            .prompt_agent(
                &self.target,
                text,
                self.patience,
                // All three are resting points. `idle` is here because of what
                // a real agent actually does: Claude Code returns to `idle`
                // when it has answered, **not** to `done`. Waiting only on
                // `done` and `blocked` meant a finished agent was never
                // noticed and every prompt expired instead — the first thing a
                // live run caught. `blocked` is the one that matters remotely:
                // without it a waiting agent looks merely slow.
                &[
                    PaneAgentState::Idle,
                    PaneAgentState::Blocked,
                    PaneAgentState::Done,
                ],
            )
            .await
            .map_err(unavailable)?;

        let output = self
            .client
            .read_agent(&self.target, self.lines)
            .await
            .map_err(unavailable)?;
        let output = Payload::text(output);

        Ok(match settled {
            // Done and idle both mean the agent has nothing outstanding.
            PaneAgentState::Done | PaneAgentState::Idle => AgentOutcome::finished(output),
            PaneAgentState::Blocked => AgentOutcome::blocked(output),
            // Still going when patience ran out.
            PaneAgentState::Working => AgentOutcome::still_working(output),
            // Herdr could not tell. Saying "finished" would invite someone to
            // act on a half-answer; "still working" is the conservative read
            // and the one that does not claim completion.
            PaneAgentState::Unknown => AgentOutcome::still_working(output),
        })
    }
}

/// Every client failure here is the runtime being unreachable or unhappy.
fn unavailable(error: ClientError) -> AgentError {
    AgentError::Unavailable {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use kamiroh_domain::AgentStatus;

    use super::*;
    use crate::client::tests::FakeHerdr;

    /// A prompt reply carrying `agent_status`, as `agent.prompt` returns.
    fn prompted(status: &str) -> String {
        format!(r#"{{"id":"1","result":{{"agent":{{"agent_status":"{status}"}}}}}}"#)
    }

    /// Herdr answers each request on its own connection, so a run needs two
    /// scripted replies: one for `agent.prompt`, one for `agent.read`.
    async fn run_against(replies: Vec<String>, prompt: &str) -> Result<AgentOutcome, AgentError> {
        let herdr = FakeHerdr::scripted(replies).await;
        let agent = HerdrAgent::new(Client::new(herdr.path()), "w1:p1");
        let outcome = agent.run(Payload::text(prompt)).await;
        drop(herdr);
        outcome
    }

    fn read(text: &str) -> String {
        format!(r#"{{"id":"2","result":{{"read":{{"text":"{text}"}}}}}}"#)
    }

    #[tokio::test]
    async fn a_finished_agent_produces_its_output() {
        let outcome = run_against(vec![prompted("done"), read("all done")], "build it")
            .await
            .unwrap();

        assert_eq!(outcome.status, AgentStatus::Idle);
        assert_eq!(outcome.output.as_text(), Some("all done"));
        assert!(outcome.is_finished());
    }

    /// The case the whole `Blocked` vocabulary exists for.
    #[tokio::test]
    async fn a_blocked_agent_is_reported_blocked_with_its_question() {
        let outcome = run_against(
            vec![prompted("blocked"), read("Run rm -rf build? (y/n)")],
            "clean up",
        )
        .await
        .unwrap();

        assert_eq!(outcome.status, AgentStatus::Blocked);
        assert_eq!(outcome.output.as_text(), Some("Run rm -rf build? (y/n)"));
        assert!(!outcome.is_finished(), "blocked is not finished");
    }

    #[tokio::test]
    async fn an_agent_still_working_when_patience_ran_out_says_so() {
        let outcome = run_against(vec![prompted("working"), read("step 3 of 9")], "big job")
            .await
            .unwrap();

        assert_eq!(outcome.status, AgentStatus::Busy);
        assert_eq!(outcome.output.as_text(), Some("step 3 of 9"));
    }

    /// Herdr reports an expired wait as an error. Treating it as one made a
    /// slow agent indistinguishable from a broken socket — the first thing the
    /// live run against a real agent hit.
    #[tokio::test]
    async fn an_expired_wait_is_still_working_not_a_failure() {
        let timeout = r#"{"id":"1","error":{"code":"timeout","message":"timed out waiting for agent status"}}"#;
        let outcome = run_against(vec![timeout.to_owned(), read("half an answer")], "slow job")
            .await
            .expect("an expired wait must not fail the run");

        assert_eq!(outcome.status, AgentStatus::Busy);
        assert_eq!(outcome.output.as_text(), Some("half an answer"));
    }

    /// Any *other* refusal is still a failure — the mapping above must not
    /// swallow real errors.
    #[tokio::test]
    async fn a_non_timeout_refusal_is_still_an_error() {
        let refused = r#"{"id":"1","error":{"code":"agent_not_found","message":"agent target w1:p1 not found"}}"#;
        let error = run_against(vec![refused.to_owned()], "hello")
            .await
            .unwrap_err();

        assert!(
            matches!(error, AgentError::Unavailable { .. }),
            "got {error:?}"
        );
    }

    /// Herdr not knowing must not become kamiroh claiming completion.
    #[tokio::test]
    async fn an_unknown_state_is_treated_as_still_working() {
        let outcome = run_against(vec![prompted("unknown"), read("...")], "who knows")
            .await
            .unwrap();
        assert_eq!(outcome.status, AgentStatus::Busy);
    }

    /// An infrastructure failure must not arrive looking like agent output.
    #[tokio::test]
    async fn an_unreachable_runtime_is_an_error_not_an_empty_answer() {
        let dir = tempfile::tempdir().unwrap();
        let agent = HerdrAgent::new(Client::new(dir.path().join("absent.sock")), "w1:p1");

        let error = agent.run(Payload::text("hello")).await.unwrap_err();
        assert!(
            matches!(error, AgentError::Unavailable { .. }),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn a_non_text_prompt_is_refused_rather_than_typed_at_a_terminal() {
        let herdr = FakeHerdr::scripted(vec![]).await;
        let agent = HerdrAgent::new(Client::new(herdr.path()), "w1:p1");

        let error = agent
            .run(Payload::new("image/png", vec![0x89, 0x50, 0x4e, 0x47]))
            .await
            .unwrap_err();

        assert!(
            matches!(error, AgentError::Unsupported { .. }),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn the_prompt_text_and_target_reach_herdr() {
        let herdr = FakeHerdr::scripted(vec![prompted("done"), read("ok")]).await;
        let agent = HerdrAgent::new(Client::new(herdr.path()), "w7:p3");
        agent.run(Payload::text("do the thing")).await.unwrap();

        let sent = herdr.requests().await;
        assert_eq!(sent[0]["method"], "agent.prompt");
        assert_eq!(sent[0]["params"]["target"], "w7:p3");
        assert_eq!(sent[0]["params"]["text"], "do the thing");
        // `idle` must be in the list. A real Claude agent returns to `idle`
        // when it has answered, not to `done`, so omitting it meant every
        // prompt expired instead of completing.
        let until = sent[0]["params"]["wait"]["until"]
            .as_array()
            .expect("until must be a list");
        for state in ["idle", "blocked", "done"] {
            assert!(
                until.iter().any(|value| value == state),
                "{state} missing from {until:?}"
            );
        }

        assert_eq!(sent[1]["method"], "agent.read");
        assert_eq!(sent[1]["params"]["target"], "w7:p3");
        assert_eq!(sent[1]["params"]["source"], "recent");
    }

    #[tokio::test]
    async fn patience_is_sent_as_the_wait_timeout() {
        let herdr = FakeHerdr::scripted(vec![prompted("done"), read("ok")]).await;
        let agent = HerdrAgent::new(Client::new(herdr.path()), "w1:p1")
            .with_patience(Duration::from_secs(5));
        agent.run(Payload::text("x")).await.unwrap();

        let sent = herdr.requests().await;
        assert_eq!(sent[0]["params"]["wait"]["timeout_ms"], 5000);
    }

    /// The front gives a request 30s; waiting longer means the caller is
    /// answered by a timeout rather than by the agent.
    #[test]
    fn default_patience_leaves_room_under_the_front_timeout() {
        assert!(
            DEFAULT_PATIENCE < Duration::from_secs(30),
            "default patience must stay under the Iroh front's request timeout"
        );
    }
}
