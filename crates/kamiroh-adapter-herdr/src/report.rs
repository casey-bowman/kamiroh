//! Reporting the pane's agent state into Herdr's pane list.
//!
//! # Why this decorates [`Link`] and not `AgentController`
//!
//! The obvious place is a decorator over the `AgentController` port, which sees
//! every message any front delivers. It is the wrong place. A pane on a laptop
//! driving an agent on the home node never touches the local controller at all
//! — the messages leave over `Transport` — so that decorator would report
//! nothing in exactly the case worth reporting.
//!
//! A pane's state is the state of the agent *that pane is bound to*, which is
//! what [`Link`] names. Decorating it covers local and remote uniformly, and
//! changes no port.
//!
//! # What this does not cover
//!
//! A node serving agents for remote peers has nobody at its pane, and inbound
//! messages arrive through the Iroh front rather than through any `Link`. Its
//! pane will not show its agent working while a peer drives it. That wants the
//! `AgentController` decorator after all, as a second reporter — Herdr's
//! `pane.report_agent` takes an optional `seq`, which is the mechanism for
//! ordering two sources reporting on one pane.
//!
//! # Reporting never delays or fails a control message
//!
//! States go to a background task through a bounded channel, and a full channel
//! drops the update rather than waiting. A sidebar label is not worth making
//! someone's prompt slower, and Herdr being down is not a reason for kamiroh to
//! stop working.

use std::sync::Arc;

use async_trait::async_trait;
use kamiroh_domain::{ActorName, ControlMessage, ControlReply};
use tokio::sync::mpsc;

use crate::link::{Link, LinkError};
use crate::pane::{Pane, PaneAgentState};

/// How many pending state updates before the oldest are dropped.
///
/// Small on purpose: these are labels, and a stale one is worth less than a
/// fresh one. Anything beyond a handful means the socket is not draining, in
/// which case the updates are already worthless.
const BACKLOG: usize = 16;

/// A [`Link`] that reports what it is doing to Herdr.
pub struct ReportingLink {
    inner: Arc<dyn Link>,
    reports: mpsc::Sender<PaneAgentState>,
}

impl ReportingLink {
    /// Wraps `inner`, sending state updates to `reports`.
    pub(crate) fn new(inner: Arc<dyn Link>, reports: mpsc::Sender<PaneAgentState>) -> Self {
        Self { inner, reports }
    }

    /// Sends a state update, or drops it. Never blocks, never fails.
    fn report(&self, state: PaneAgentState) {
        let _ = self.reports.try_send(state);
    }
}

#[async_trait]
impl Link for ReportingLink {
    async fn send(&self, message: ControlMessage) -> Result<ControlReply, LinkError> {
        let kind = Kind::of(&message);

        // Only a prompt means the agent is about to work. Asking for status
        // must not itself claim the agent is busy — that would make every
        // `/status` briefly lie.
        if matches!(kind, Kind::Prompt) {
            self.report(PaneAgentState::Working);
        }

        let result = self.inner.send(message).await;
        self.report(state_after(kind, &result));
        result
    }

    fn describe(&self) -> String {
        self.inner.describe()
    }
}

/// Which control verb was sent, kept because the message itself is moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Prompt,
    Status,
    Interrupt,
    Shutdown,
}

impl Kind {
    fn of(message: &ControlMessage) -> Self {
        match message {
            ControlMessage::Prompt(_) => Self::Prompt,
            ControlMessage::Status => Self::Status,
            ControlMessage::Interrupt => Self::Interrupt,
            ControlMessage::Shutdown => Self::Shutdown,
        }
    }
}

/// The state to report once a message has been answered.
fn state_after(kind: Kind, result: &Result<ControlReply, LinkError>) -> PaneAgentState {
    match result {
        // A peer we could not reach tells us nothing about the agent behind it.
        // Reporting `idle` here would be a guess presented as a fact.
        Err(_) => PaneAgentState::Unknown,
        // The agent said what it is doing; nothing beats asking.
        Ok(ControlReply::Status(status)) => (*status).into(),
        // A partial reply carries the reason it is partial. This is the case
        // the whole `Partial` variant exists for: without it a blocked agent
        // came back as `Output` and got reported `idle` — a guess presented as
        // a fact, which is what §6d forbids.
        Ok(ControlReply::Partial { status, .. }) => (*status).into(),
        Ok(ControlReply::Output(_)) => PaneAgentState::Idle,
        Ok(ControlReply::Accepted) => match kind {
            Kind::Shutdown => PaneAgentState::Done,
            Kind::Prompt | Kind::Status | Kind::Interrupt => PaneAgentState::Idle,
        },
    }
}

/// Wraps `link` so it reports to Herdr, if this process is in a pane.
///
/// Returns the link to use and a line describing what was arranged. Outside a
/// pane the link is returned untouched: kamiroh runs outside Herdr as a matter
/// of course, and that is not a degraded mode worth warning about.
#[cfg(unix)]
pub fn attach(link: Arc<dyn Link>, agent: &ActorName) -> (Arc<dyn Link>, String) {
    let Some(pane) = Pane::from_env() else {
        return (link, "not in a Herdr pane; not reporting".to_owned());
    };

    let (reports, receiver) = mpsc::channel(BACKLOG);
    let summary = format!(
        "reporting {agent} to pane {} via {}",
        pane.id,
        pane.socket.display()
    );

    // An opening state claims the pane for kamiroh. Herdr falls back to reading
    // the terminal when nothing reports, and would otherwise label this pane by
    // whatever kamiroh's output happens to look like.
    let _ = reports.try_send(PaneAgentState::Idle);

    tokio::spawn(run(pane, agent.to_string(), receiver));
    (Arc::new(ReportingLink::new(link, reports)), summary)
}

/// Non-Unix: Herdr's socket is a named pipe there, which this does not speak.
#[cfg(not(unix))]
pub fn attach(link: Arc<dyn Link>, _agent: &ActorName) -> (Arc<dyn Link>, String) {
    (
        link,
        "Herdr reporting is Unix-only; not reporting".to_owned(),
    )
}

/// Drains state updates onto the socket.
///
/// Each report opens its own connection, because Herdr closes one after every
/// response — so there is no connection state to keep here, and no reconnect
/// logic to get wrong.
#[cfg(unix)]
async fn run(pane: Pane, agent: String, mut reports: mpsc::Receiver<PaneAgentState>) {
    let client = crate::client::Client::new(&pane.socket);
    // Herdr going away should cost one line, not one line per update. Reset on
    // success so that a later outage is reported again.
    let mut complained = false;

    while let Some(state) = reports.recv().await {
        match client.report_agent(&pane.id, &agent, state).await {
            Ok(()) => complained = false,
            Err(error) => {
                if !complained {
                    eprintln!("kamiroh: not reporting to Herdr: {error}");
                    complained = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kamiroh_domain::{AgentStatus, Payload};
    use kamiroh_ports::{ControlApiError, ControllerError};

    use super::*;

    /// A link that answers however the test says.
    struct StubLink(Result<ControlReply, ()>);

    #[async_trait]
    impl Link for StubLink {
        async fn send(&self, _message: ControlMessage) -> Result<ControlReply, LinkError> {
            self.0.clone().map_err(|()| {
                LinkError::Local(ControlApiError::Controller(ControllerError::NoSuchActor {
                    actor: "agent".to_owned(),
                }))
            })
        }

        fn describe(&self) -> String {
            "agent on this node".to_owned()
        }
    }

    /// Sends `message` through a reporting link and returns what was reported.
    async fn reported(
        message: ControlMessage,
        answer: Result<ControlReply, ()>,
    ) -> Vec<PaneAgentState> {
        let (sender, mut receiver) = mpsc::channel(BACKLOG);
        let link = ReportingLink::new(Arc::new(StubLink(answer)), sender);
        let _ = link.send(message).await;
        drop(link);

        let mut seen = Vec::new();
        while let Some(state) = receiver.recv().await {
            seen.push(state);
        }
        seen
    }

    #[tokio::test]
    async fn a_prompt_reports_working_then_idle() {
        let seen = reported(
            ControlMessage::Prompt(Payload::text("go")),
            Ok(ControlReply::Output(Payload::text("done"))),
        )
        .await;
        assert_eq!(seen, vec![PaneAgentState::Working, PaneAgentState::Idle]);
    }

    /// Asking what an agent is doing must not itself claim it is doing it.
    #[tokio::test]
    async fn a_status_request_does_not_report_working() {
        let seen = reported(
            ControlMessage::Status,
            Ok(ControlReply::Status(AgentStatus::Idle)),
        )
        .await;
        assert_eq!(seen, vec![PaneAgentState::Idle]);
    }

    #[tokio::test]
    async fn a_status_reply_is_reported_as_the_agent_described_it() {
        let seen = reported(
            ControlMessage::Status,
            Ok(ControlReply::Status(AgentStatus::Busy)),
        )
        .await;
        assert_eq!(seen, vec![PaneAgentState::Working]);
    }

    #[tokio::test]
    async fn shutdown_reports_done() {
        let seen = reported(ControlMessage::Shutdown, Ok(ControlReply::Accepted)).await;
        assert_eq!(seen, vec![PaneAgentState::Done]);
    }

    #[tokio::test]
    async fn interrupt_reports_idle_rather_than_done() {
        let seen = reported(ControlMessage::Interrupt, Ok(ControlReply::Accepted)).await;
        assert_eq!(seen, vec![PaneAgentState::Idle]);
    }

    /// An unreachable peer says nothing about the agent behind it.
    #[tokio::test]
    async fn a_failed_send_reports_unknown_not_idle() {
        let seen = reported(ControlMessage::Prompt(Payload::text("go")), Err(())).await;
        assert_eq!(seen, vec![PaneAgentState::Working, PaneAgentState::Unknown]);
    }

    #[tokio::test]
    async fn the_reply_is_passed_through_unchanged() {
        let (sender, _receiver) = mpsc::channel(BACKLOG);
        let link = ReportingLink::new(
            Arc::new(StubLink(Ok(ControlReply::Output(Payload::text("hi"))))),
            sender,
        );

        let reply = link.send(ControlMessage::Status).await.unwrap();
        assert_eq!(reply, ControlReply::Output(Payload::text("hi")));
    }

    /// A stalled reporter must not stall the person typing.
    #[tokio::test]
    async fn a_full_backlog_drops_updates_instead_of_blocking() {
        let (sender, receiver) = mpsc::channel(1);
        let link = ReportingLink::new(
            Arc::new(StubLink(Ok(ControlReply::Accepted))),
            sender.clone(),
        );

        // Nobody is draining `receiver`, so the channel fills immediately.
        for _ in 0..50 {
            link.send(ControlMessage::Interrupt).await.unwrap();
        }

        drop(receiver);
        // Still usable afterwards: dropping updates is not a poisoned state.
        assert!(link.send(ControlMessage::Interrupt).await.is_ok());
    }

    #[tokio::test]
    async fn describe_is_the_wrapped_links_own() {
        let (sender, _receiver) = mpsc::channel(BACKLOG);
        let link = ReportingLink::new(Arc::new(StubLink(Ok(ControlReply::Accepted))), sender);
        assert_eq!(link.describe(), "agent on this node");
    }

    #[test]
    fn every_control_verb_has_a_kind() {
        assert_eq!(
            Kind::of(&ControlMessage::Prompt(Payload::text("x"))),
            Kind::Prompt
        );
        assert_eq!(Kind::of(&ControlMessage::Status), Kind::Status);
        assert_eq!(Kind::of(&ControlMessage::Interrupt), Kind::Interrupt);
        assert_eq!(Kind::of(&ControlMessage::Shutdown), Kind::Shutdown);
    }
}
