//! In-memory [`AgentController`] that echoes prompts back.
//!
//! Since slice G the node runs `kamiroh-adapter-kameo`; this is the test double
//! that outlived it. It holds agent state in a map rather than an actor, which
//! is the point: a test for a *front* wants a controller that answers
//! immediately and needs no runtime, not one with a mailbox and a lifecycle of
//! its own. `AgentStatus::Busy` is unreachable here, so tests needing a
//! genuinely working agent belong with the Kameo adapter.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use kamiroh_domain::{ActorName, AgentStatus, ControlMessage, ControlReply};
use kamiroh_ports::{AgentController, ControllerError};

/// A controller holding agent state in memory.
#[derive(Debug, Default)]
pub struct EchoController {
    agents: Mutex<HashMap<ActorName, AgentStatus>>,
}

impl EchoController {
    /// Creates a controller with no agents registered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a controller with `agents` registered and idle.
    pub fn with_agents(agents: impl IntoIterator<Item = ActorName>) -> Self {
        Self {
            agents: Mutex::new(
                agents
                    .into_iter()
                    .map(|name| (name, AgentStatus::Idle))
                    .collect(),
            ),
        }
    }

    /// Registers `agent` as idle.
    pub fn register(&self, agent: ActorName) {
        self.agents
            .lock()
            .expect("controller lock poisoned")
            .insert(agent, AgentStatus::Idle);
    }

    /// Reports `agent`'s status, if it is registered.
    pub fn status(&self, agent: &ActorName) -> Option<AgentStatus> {
        self.agents
            .lock()
            .expect("controller lock poisoned")
            .get(agent)
            .copied()
    }
}

#[async_trait]
impl AgentController for EchoController {
    async fn dispatch(
        &self,
        agent: &ActorName,
        message: ControlMessage,
    ) -> Result<ControlReply, ControllerError> {
        let mut agents = self.agents.lock().expect("controller lock poisoned");
        let status = agents
            .get_mut(agent)
            .ok_or_else(|| ControllerError::NoSuchActor {
                actor: agent.to_string(),
            })?;

        if *status == AgentStatus::Stopped {
            return Err(ControllerError::Stopped {
                actor: agent.to_string(),
            });
        }

        match message {
            ControlMessage::Prompt(payload) => Ok(ControlReply::Output(payload)),
            ControlMessage::Status => Ok(ControlReply::Status(*status)),
            ControlMessage::Interrupt => {
                *status = AgentStatus::Idle;
                Ok(ControlReply::Accepted)
            }
            ControlMessage::Detach => {
                *status = AgentStatus::Stopped;
                Ok(ControlReply::Accepted)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kamiroh_domain::Payload;

    use super::*;

    fn agent() -> ActorName {
        ActorName::new("agent").unwrap()
    }

    #[tokio::test]
    async fn prompt_is_echoed_back_as_output() {
        let controller = EchoController::with_agents([agent()]);
        let reply = controller
            .dispatch(&agent(), ControlMessage::Prompt(Payload::text("ping")))
            .await
            .unwrap();
        assert_eq!(reply, ControlReply::Output(Payload::text("ping")));
    }

    #[tokio::test]
    async fn unregistered_agent_is_not_found() {
        let controller = EchoController::new();
        let error = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap_err();
        assert!(matches!(error, ControllerError::NoSuchActor { .. }));
    }

    #[tokio::test]
    async fn detaching_stops_the_actor_and_later_messages_fail() {
        let controller = EchoController::with_agents([agent()]);

        assert_eq!(
            controller
                .dispatch(&agent(), ControlMessage::Detach)
                .await
                .unwrap(),
            ControlReply::Accepted
        );
        assert_eq!(controller.status(&agent()), Some(AgentStatus::Stopped));

        let error = controller
            .dispatch(&agent(), ControlMessage::Status)
            .await
            .unwrap_err();
        assert!(matches!(error, ControllerError::Stopped { .. }));
    }

    #[tokio::test]
    async fn interrupt_returns_the_agent_to_idle() {
        let controller = EchoController::with_agents([agent()]);
        assert_eq!(
            controller
                .dispatch(&agent(), ControlMessage::Interrupt)
                .await
                .unwrap(),
            ControlReply::Accepted
        );
        assert_eq!(controller.status(&agent()), Some(AgentStatus::Idle));
    }
}
