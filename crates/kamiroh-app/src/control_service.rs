//! The one use case that exists so far: authorise a control message, then
//! deliver it to the agent's controller actor.

use std::sync::Arc;

use async_trait::async_trait;
use kamiroh_domain::{ActorName, ControlMessage, ControlReply};
use kamiroh_ports::{AgentController, Allowlist, ControlApi, ControlApiError, Origin};

/// Implements the driving [`ControlApi`] port on top of driven ports.
///
/// It holds `Arc<dyn Port>` rather than generics so that every front shares one
/// instance, and so the composition root can swap adapters without the app layer
/// changing shape.
pub struct ControlService {
    allowlist: Arc<dyn Allowlist>,
    controller: Arc<dyn AgentController>,
}

impl ControlService {
    /// Wires the service to an allowlist and a controller.
    pub fn new(allowlist: Arc<dyn Allowlist>, controller: Arc<dyn AgentController>) -> Self {
        Self {
            allowlist,
            controller,
        }
    }
}

#[async_trait]
impl ControlApi for ControlService {
    async fn deliver(
        &self,
        origin: Origin,
        agent: &ActorName,
        message: ControlMessage,
    ) -> Result<ControlReply, ControlApiError> {
        // Authorise before anything else: no lookup, no dispatch, and no error
        // that distinguishes "unknown agent" from "not allowed" may happen for a
        // caller we have not admitted. `remote_endpoint()` returning `Some` is
        // exactly the set of messages needing authorisation; `None` means a
        // front inside the trust boundary built this with `Origin::local_front`.
        if let Some(endpoint) = origin.remote_endpoint()
            && !self.allowlist.is_allowed(&endpoint)
        {
            return Err(ControlApiError::NotAllowed { endpoint });
        }

        Ok(self.controller.dispatch(agent, message).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kamiroh_domain::{AgentStatus, EndpointId, Payload};
    use kamiroh_ports::ControllerError;

    use super::*;

    /// Allows exactly the endpoints it was built with.
    struct FakeAllowlist(Vec<EndpointId>);

    impl Allowlist for FakeAllowlist {
        fn is_allowed(&self, endpoint: &EndpointId) -> bool {
            self.0.contains(endpoint)
        }
    }

    /// Records every dispatch it receives.
    #[derive(Default)]
    struct RecordingController {
        seen: Mutex<Vec<(ActorName, ControlMessage)>>,
    }

    impl RecordingController {
        fn seen(&self) -> Vec<(ActorName, ControlMessage)> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AgentController for RecordingController {
        async fn dispatch(
            &self,
            agent: &ActorName,
            message: ControlMessage,
        ) -> Result<ControlReply, ControllerError> {
            self.seen.lock().unwrap().push((agent.clone(), message));
            Ok(ControlReply::Status(AgentStatus::Idle))
        }
    }

    fn endpoint(byte: u8) -> EndpointId {
        EndpointId::from_bytes([byte; 32])
    }

    fn agent() -> ActorName {
        ActorName::new("agent").unwrap()
    }

    fn service(allowed: Vec<EndpointId>) -> (ControlService, Arc<RecordingController>) {
        let controller = Arc::new(RecordingController::default());
        let service = ControlService::new(
            Arc::new(FakeAllowlist(allowed)),
            controller.clone() as Arc<dyn AgentController>,
        );
        (service, controller)
    }

    #[tokio::test]
    async fn allowed_remote_endpoint_reaches_the_controller() {
        let (service, controller) = service(vec![endpoint(1)]);

        let reply = service
            .deliver(
                Origin::remote(endpoint(1)),
                &agent(),
                ControlMessage::Status,
            )
            .await
            .unwrap();

        assert_eq!(reply, ControlReply::Status(AgentStatus::Idle));
        assert_eq!(controller.seen().len(), 1);
    }

    #[tokio::test]
    async fn unlisted_remote_endpoint_is_rejected_without_dispatching() {
        let (service, controller) = service(vec![endpoint(1)]);

        let error = service
            .deliver(
                Origin::remote(endpoint(2)),
                &agent(),
                ControlMessage::Prompt(Payload::text("do the thing")),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ControlApiError::NotAllowed { endpoint: e } if e == endpoint(2)
        ));
        // The security property that matters: nothing reached the agent.
        assert!(controller.seen().is_empty());
    }

    #[tokio::test]
    async fn empty_allowlist_rejects_every_remote_endpoint() {
        let (service, controller) = service(vec![]);

        for byte in [0, 1, 42, 255] {
            let error = service
                .deliver(
                    Origin::remote(endpoint(byte)),
                    &agent(),
                    ControlMessage::Shutdown,
                )
                .await
                .unwrap_err();
            assert!(matches!(error, ControlApiError::NotAllowed { .. }));
        }
        assert!(controller.seen().is_empty());
    }

    #[tokio::test]
    async fn local_origin_bypasses_the_allowlist() {
        // A Herdr pane on this node is already inside the trust boundary; it must
        // work even though the allowlist is empty.
        let (service, controller) = service(vec![]);

        service
            .deliver(Origin::local_front(), &agent(), ControlMessage::Interrupt)
            .await
            .unwrap();

        assert_eq!(controller.seen().len(), 1);
    }

    #[tokio::test]
    async fn controller_failures_surface_unchanged() {
        struct FailingController;

        #[async_trait]
        impl AgentController for FailingController {
            async fn dispatch(
                &self,
                agent: &ActorName,
                _message: ControlMessage,
            ) -> Result<ControlReply, ControllerError> {
                Err(ControllerError::NoSuchActor {
                    actor: agent.to_string(),
                })
            }
        }

        let service = ControlService::new(
            Arc::new(FakeAllowlist(vec![endpoint(1)])),
            Arc::new(FailingController),
        );

        let error = service
            .deliver(
                Origin::remote(endpoint(1)),
                &agent(),
                ControlMessage::Status,
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ControlApiError::Controller(ControllerError::NoSuchActor { .. })
        ));
    }
}
