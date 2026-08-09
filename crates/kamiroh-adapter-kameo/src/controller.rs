//! [`KameoController`] — the [`AgentController`] port over Kameo actors.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::error::SendError;
use kamiroh_domain::{ActorName, ControlMessage, ControlReply};
use kamiroh_ports::{AgentController, ControllerError};

use crate::actor::AgentActor;
use crate::agent::Agent;

/// Routes control messages to one controller actor per agent.
///
/// Replaces `kamiroh-adapter-memory`'s `EchoController`. The visible difference
/// is that an agent's state now lives in an actor rather than in a map: only
/// one message is handled at a time, `Shutdown` really stops something, and
/// [`AgentStatus::Busy`](kamiroh_domain::AgentStatus::Busy) is reachable
/// because a prompt genuinely takes time.
#[derive(Default)]
pub struct KameoController {
    agents: HashMap<ActorName, ActorRef<AgentActor>>,
}

impl KameoController {
    /// Creates a controller with no agents.
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawns a controller actor for `name`, driving `agent`.
    ///
    /// Replacing an existing name drops this controller's reference to the old
    /// actor, which stops it once nothing else holds one.
    ///
    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime, since spawning an actor spawns
    /// a task.
    pub fn spawn(&mut self, name: ActorName, agent: impl Agent) {
        let actor = AgentActor::spawn(AgentActor::new(name.clone(), Arc::new(agent)));
        self.agents.insert(name, actor);
    }

    /// Builder form of [`spawn`](Self::spawn), for wiring in one expression.
    ///
    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime.
    #[must_use]
    pub fn with_agent(mut self, name: ActorName, agent: impl Agent) -> Self {
        self.spawn(name, agent);
        self
    }

    /// Whether `agent` has a controller actor that is still running.
    pub fn is_running(&self, agent: &ActorName) -> bool {
        self.agents
            .get(agent)
            .is_some_and(kameo::actor::ActorRef::is_alive)
    }
}

#[async_trait]
impl AgentController for KameoController {
    async fn dispatch(
        &self,
        agent: &ActorName,
        message: ControlMessage,
    ) -> Result<ControlReply, ControllerError> {
        let actor = self
            .agents
            .get(agent)
            .ok_or_else(|| ControllerError::NoSuchActor {
                actor: agent.to_string(),
            })?;

        actor
            .ask(message)
            .await
            .map_err(|error| translate(agent, error))
    }
}

/// Maps a Kameo delivery failure onto the port's vocabulary.
///
/// Kameo's error carries the undelivered message back; it is dropped here
/// rather than reported, since a control message can hold a prompt payload and
/// an error is a poor place to keep one.
fn translate(
    agent: &ActorName,
    error: SendError<ControlMessage, ControllerError>,
) -> ControllerError {
    match error {
        // Both mean the actor is gone. The distinction Kameo draws — never
        // started versus stopped mid-ask — is not one the port makes, and a
        // caller cannot act on it differently.
        SendError::ActorNotRunning(_) | SendError::ActorStopped => ControllerError::Stopped {
            actor: agent.to_string(),
        },
        SendError::MailboxFull(_) => ControllerError::Rejected {
            actor: agent.to_string(),
            reason: "the controller actor's mailbox is full".to_owned(),
        },
        // The actor answered, and it answered with the port's own error.
        SendError::HandlerError(error) => error,
        SendError::Timeout(_) => ControllerError::Timeout {
            actor: agent.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use kamiroh_domain::{AgentStatus, Payload};
    use tokio::sync::Notify;

    use super::*;
    use crate::agent::EchoAgent;

    fn agent() -> ActorName {
        ActorName::new("agent").unwrap()
    }

    fn other() -> ActorName {
        ActorName::new("other").unwrap()
    }

    /// An agent that blocks until the test lets it finish, so a prompt can be
    /// observed mid-flight. Everything below that involves `Busy` or
    /// `Interrupt` needs work that is genuinely in progress.
    struct GatedAgent {
        /// Notified once `run` has been entered.
        started: Arc<Notify>,
        /// Awaited inside `run`; the test decides when the prompt finishes.
        release: Arc<Notify>,
        /// Counts prompts that ran to completion — zero after a real abort.
        completed: Arc<AtomicUsize>,
    }

    /// A gated agent and the handles a test drives it with.
    struct Gate {
        started: Arc<Notify>,
        release: Arc<Notify>,
        completed: Arc<AtomicUsize>,
    }

    impl Gate {
        fn new() -> (Self, GatedAgent) {
            let gate = Self {
                started: Arc::new(Notify::new()),
                release: Arc::new(Notify::new()),
                completed: Arc::new(AtomicUsize::new(0)),
            };
            let agent = GatedAgent {
                started: Arc::clone(&gate.started),
                release: Arc::clone(&gate.release),
                completed: Arc::clone(&gate.completed),
            };
            (gate, agent)
        }
    }

    #[async_trait]
    impl Agent for GatedAgent {
        async fn run(&self, prompt: Payload) -> Payload {
            self.started.notify_one();
            self.release.notified().await;
            self.completed.fetch_add(1, Ordering::SeqCst);
            prompt
        }
    }

    /// Sends `message` to `agent` from another task, so the test can keep
    /// talking to the controller while that message is outstanding.
    fn dispatch_concurrently(
        controller: &Arc<KameoController>,
        agent: ActorName,
        message: ControlMessage,
    ) -> tokio::task::JoinHandle<Result<ControlReply, ControllerError>> {
        let controller = Arc::clone(controller);
        tokio::spawn(async move { controller.dispatch(&agent, message).await })
    }

    #[tokio::test]
    async fn a_prompt_reaches_the_agent_and_its_output_comes_back() {
        let controller = KameoController::new().with_agent(agent(), EchoAgent);

        let reply = controller
            .dispatch(&agent(), ControlMessage::Prompt(Payload::text("ping")))
            .await
            .unwrap();

        assert_eq!(reply, ControlReply::Output(Payload::text("ping")));
    }

    #[tokio::test]
    async fn a_fresh_agent_is_idle() {
        let controller = KameoController::new().with_agent(agent(), EchoAgent);

        let reply = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap();

        assert_eq!(reply, ControlReply::Status(AgentStatus::Idle));
    }

    #[tokio::test]
    async fn an_unregistered_agent_has_no_actor() {
        let controller = KameoController::new();

        let error = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap_err();

        assert!(matches!(error, ControllerError::NoSuchActor { .. }));
    }

    /// The point of the slice: `Busy` was unreachable while the controller was
    /// a `HashMap`, because nothing ever took time.
    #[tokio::test]
    async fn an_agent_running_a_prompt_reports_busy() {
        let (gate, gated) = Gate::new();
        let controller = Arc::new(KameoController::new().with_agent(agent(), gated));

        let pending = dispatch_concurrently(
            &controller,
            agent(),
            ControlMessage::Prompt(Payload::text("work")),
        );
        gate.started.notified().await;

        let status = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap();
        assert_eq!(status, ControlReply::Status(AgentStatus::Busy));

        gate.release.notify_one();
        assert_eq!(
            pending.await.unwrap().unwrap(),
            ControlReply::Output(Payload::text("work"))
        );

        let status = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap();
        assert_eq!(status, ControlReply::Status(AgentStatus::Idle));
    }

    #[tokio::test]
    async fn interrupt_abandons_the_running_prompt_and_returns_the_agent_to_idle() {
        let (gate, gated) = Gate::new();
        let controller = Arc::new(KameoController::new().with_agent(agent(), gated));

        let pending = dispatch_concurrently(
            &controller,
            agent(),
            ControlMessage::Prompt(Payload::text("work")),
        );
        gate.started.notified().await;

        assert_eq!(
            controller
                .dispatch(&agent(), ControlMessage::Interrupt)
                .await
                .unwrap(),
            ControlReply::Accepted
        );

        // The caller waiting on the prompt is told, rather than left hanging.
        let error = pending.await.unwrap().unwrap_err();
        assert!(matches!(error, ControllerError::Rejected { .. }));

        assert_eq!(
            controller
                .dispatch(&agent(), ControlMessage::Status)
                .await
                .unwrap(),
            ControlReply::Status(AgentStatus::Idle)
        );

        // Releasing now proves the abort was real: a merely-detached task would
        // wake here and count a completion.
        gate.release.notify_one();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(gate.completed.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_second_prompt_is_refused_while_one_is_running() {
        let (gate, gated) = Gate::new();
        let controller = Arc::new(KameoController::new().with_agent(agent(), gated));

        let pending = dispatch_concurrently(
            &controller,
            agent(),
            ControlMessage::Prompt(Payload::text("first")),
        );
        gate.started.notified().await;

        let error = controller
            .dispatch(&agent(), ControlMessage::Prompt(Payload::text("second")))
            .await
            .unwrap_err();
        assert!(matches!(error, ControllerError::Rejected { .. }));

        // The first prompt is untouched by the refusal of the second.
        gate.release.notify_one();
        assert_eq!(
            pending.await.unwrap().unwrap(),
            ControlReply::Output(Payload::text("first"))
        );
    }

    #[tokio::test]
    async fn shutdown_stops_the_agent_and_later_messages_are_refused() {
        let controller = KameoController::new().with_agent(agent(), EchoAgent);

        assert_eq!(
            controller
                .dispatch(&agent(), ControlMessage::Shutdown)
                .await
                .unwrap(),
            ControlReply::Accepted
        );

        let error = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap_err();
        assert!(matches!(error, ControllerError::Stopped { .. }));
    }

    /// The answer must not depend on whether the actor has finished stopping,
    /// which is what the explicit `Stopped` check in the handler is for.
    #[tokio::test]
    async fn a_shut_down_agent_stays_refused_once_its_actor_is_really_gone() {
        let controller = KameoController::new().with_agent(agent(), EchoAgent);
        controller
            .dispatch(&agent(), ControlMessage::Shutdown)
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!controller.is_running(&agent()));

        let error = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap_err();
        assert!(matches!(error, ControllerError::Stopped { .. }));
    }

    #[tokio::test]
    async fn shutdown_answers_a_prompt_that_was_still_running() {
        let (gate, gated) = Gate::new();
        let controller = Arc::new(KameoController::new().with_agent(agent(), gated));

        let pending = dispatch_concurrently(
            &controller,
            agent(),
            ControlMessage::Prompt(Payload::text("work")),
        );
        gate.started.notified().await;

        controller
            .dispatch(&agent(), ControlMessage::Shutdown)
            .await
            .unwrap();

        let error = pending.await.unwrap().unwrap_err();
        assert!(matches!(error, ControllerError::Rejected { .. }));
    }

    #[tokio::test]
    async fn agents_have_separate_controllers() {
        let controller = KameoController::new()
            .with_agent(agent(), EchoAgent)
            .with_agent(other(), EchoAgent);

        controller
            .dispatch(&agent(), ControlMessage::Shutdown)
            .await
            .unwrap();

        // Stopping one agent says nothing about the other.
        assert_eq!(
            controller
                .dispatch(&other(), ControlMessage::Status)
                .await
                .unwrap(),
            ControlReply::Status(AgentStatus::Idle)
        );
    }

    /// One mailbox per agent means messages are handled in the order they were
    /// accepted, without the controller holding a lock across them.
    #[tokio::test]
    async fn messages_to_one_agent_are_handled_in_order() {
        let controller = KameoController::new().with_agent(agent(), EchoAgent);

        for i in 0..32 {
            let reply = controller
                .dispatch(
                    &agent(),
                    ControlMessage::Prompt(Payload::text(i.to_string())),
                )
                .await
                .unwrap();
            assert_eq!(reply, ControlReply::Output(Payload::text(i.to_string())));
        }

        assert_eq!(
            controller
                .dispatch(&agent(), ControlMessage::Status)
                .await
                .unwrap(),
            ControlReply::Status(AgentStatus::Idle)
        );
    }
}
