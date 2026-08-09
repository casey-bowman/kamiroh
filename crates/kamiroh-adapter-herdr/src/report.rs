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
//! # Two sources, one reporter
//!
//! A pane's own console is not the only thing that drives an agent. A node
//! serving peers has nobody at its pane, and inbound messages arrive through
//! the Iroh front — never through a `Link` — so decorating `Link` alone leaves
//! a serving node's pane permanently idle while a peer works it.
//!
//! So `AgentController` is decorated too, and the two feed **one** reporter.
//! J2's note suggested a second reporter ordered with Herdr's optional `seq`;
//! sharing a channel is better, because it removes the race rather than
//! sequencing it. One channel, one connection, one order — the order the
//! channel already imposes.
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
use kamiroh_ports::{AgentController, ControllerError};
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

/// A handle onto the one background reporter, for wrapping things.
///
/// Cheap to clone; every clone feeds the same channel, which is the point.
#[derive(Clone)]
pub struct Reporter {
    reports: mpsc::Sender<PaneAgentState>,
}

impl Reporter {
    /// Starts reporting for `agent`, if this process is in a Herdr pane.
    ///
    /// Returns the reporter and a line describing what was arranged, or `None`
    /// outside a pane — kamiroh runs outside Herdr as a matter of course, and
    /// that is not a degraded mode worth warning about.
    #[cfg(unix)]
    pub fn start(agent: &ActorName) -> Option<(Self, String)> {
        let pane = Pane::from_env()?;
        let (reports, receiver) = mpsc::channel(BACKLOG);
        let summary = format!(
            "reporting {agent} to pane {} via {}",
            pane.id,
            pane.socket.display()
        );

        // An opening state claims the pane for kamiroh. Herdr falls back to
        // reading the terminal when nothing reports, and would otherwise label
        // this pane by whatever kamiroh's output happens to look like.
        let _ = reports.try_send(PaneAgentState::Idle);

        tokio::spawn(run(pane, agent.to_string(), receiver));
        Some((Self { reports }, summary))
    }

    /// Non-Unix: Herdr's socket is a named pipe there, which this does not speak.
    #[cfg(not(unix))]
    pub fn start(_agent: &ActorName) -> Option<(Self, String)> {
        None
    }

    /// Reports what the pane's own console does with its agent.
    pub fn wrap_link(&self, link: Arc<dyn Link>) -> Arc<dyn Link> {
        Arc::new(ReportingLink::new(link, self.reports.clone()))
    }

    /// Reports what *anyone* does with this node's agents, peers included.
    ///
    /// This is the half a serving node needs: its pane has nobody at it, and
    /// the messages arrive through the Iroh front.
    /// Reports only what happens to `agent` — the one this pane shows.
    ///
    /// A node may host several; a pane displays one. Reporting all of them
    /// would have them overwrite each other in Herdr's list, which tells an
    /// operator less than reporting nothing at all.
    pub fn wrap_controller(
        &self,
        controller: Arc<dyn AgentController>,
        agent: ActorName,
    ) -> Arc<dyn AgentController> {
        Arc::new(ReportingController {
            inner: controller,
            reports: self.reports.clone(),
            reported: agent,
        })
    }
}

/// An [`AgentController`] that reports what passes through it.
struct ReportingController {
    inner: Arc<dyn AgentController>,
    reports: mpsc::Sender<PaneAgentState>,
    /// The only agent whose state this pane shows.
    reported: ActorName,
}

#[async_trait]
impl AgentController for ReportingController {
    async fn dispatch(
        &self,
        agent: &ActorName,
        message: ControlMessage,
    ) -> Result<ControlReply, ControllerError> {
        let kind = Kind::of(&message);
        let mine = *agent == self.reported;
        if mine && matches!(kind, Kind::Prompt) {
            let _ = self.reports.try_send(PaneAgentState::Working);
        }

        let result = self.inner.dispatch(agent, message).await;
        if !mine {
            return result;
        }

        // The controller's error type differs from a link's, but the question
        // is the same one: what does this reply say the agent is doing?
        let state = match &result {
            Ok(reply) => state_after(kind, &Ok(reply.clone())),
            Err(_) => PaneAgentState::Unknown,
        };
        let _ = self.reports.try_send(state);
        result
    }
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
                    tracing::warn!(pane = %pane.id, %error, "not reporting to Herdr");
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

    /// A serving node's pane must show work that arrives from a *peer*, which
    /// never touches a `Link`. This is the gap J2 left open.
    #[tokio::test]
    async fn a_controller_reports_work_that_never_touched_a_link() {
        use kamiroh_domain::ActorName;

        struct StubController;

        #[async_trait]
        impl AgentController for StubController {
            async fn dispatch(
                &self,
                _agent: &ActorName,
                _message: ControlMessage,
            ) -> Result<ControlReply, ControllerError> {
                Ok(ControlReply::Output(Payload::text("done")))
            }
        }

        let agent = ActorName::new("agent").unwrap();
        let (sender, mut receiver) = mpsc::channel(BACKLOG);
        let controller = ReportingController {
            inner: Arc::new(StubController),
            reports: sender,
            reported: agent.clone(),
        };

        controller
            .dispatch(&agent, ControlMessage::Prompt(Payload::text("go")))
            .await
            .unwrap();
        drop(controller);

        let mut seen = Vec::new();
        while let Some(state) = receiver.recv().await {
            seen.push(state);
        }
        assert_eq!(seen, vec![PaneAgentState::Working, PaneAgentState::Idle]);
    }

    /// Both halves feed one channel, so their reports cannot race — which is
    /// why no `seq` is needed to order them.
    /// A node may host several agents; a pane shows one. Work on the others
    /// must not overwrite it.
    #[tokio::test]
    async fn another_agents_work_is_not_reported_to_this_pane() {
        use kamiroh_domain::ActorName;

        struct StubController;

        #[async_trait]
        impl AgentController for StubController {
            async fn dispatch(
                &self,
                _agent: &ActorName,
                _message: ControlMessage,
            ) -> Result<ControlReply, ControllerError> {
                Ok(ControlReply::Output(Payload::text("done")))
            }
        }

        let (sender, mut receiver) = mpsc::channel(BACKLOG);
        let controller = ReportingController {
            inner: Arc::new(StubController),
            reports: sender,
            reported: ActorName::new("mine").unwrap(),
        };

        let other = ActorName::new("theirs").unwrap();
        controller
            .dispatch(&other, ControlMessage::Prompt(Payload::text("go")))
            .await
            .unwrap();
        drop(controller);

        let mut seen = Vec::new();
        while let Some(state) = receiver.recv().await {
            seen.push(state);
        }
        assert!(
            seen.is_empty(),
            "another agent's work was reported: {seen:?}"
        );
    }

    #[tokio::test]
    async fn both_decorators_share_one_stream_of_reports() {
        let (sender, mut receiver) = mpsc::channel(BACKLOG);
        let reporter = Reporter {
            reports: sender.clone(),
        };

        let link = reporter.wrap_link(Arc::new(StubLink(Ok(ControlReply::Accepted))));
        link.send(ControlMessage::Interrupt).await.unwrap();
        drop(link);
        drop(reporter);
        drop(sender);

        let mut seen = Vec::new();
        while let Some(state) = receiver.recv().await {
            seen.push(state);
        }
        assert_eq!(
            seen,
            vec![PaneAgentState::Idle],
            "the link's report arrived"
        );
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
