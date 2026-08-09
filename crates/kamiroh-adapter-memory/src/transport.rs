//! In-memory [`Transport`] that loops back to this node's own [`ControlApi`].
//!
//! It stands in for `kamiroh-adapter-iroh` and, usefully, exercises the real
//! authorisation path: a loopback send arrives via [`Origin::remote`] carrying
//! this node's endpoint id, so it is refused unless the node's allowlist
//! contains itself. That makes the composition root's smoke path a genuine test
//! of the allowlist rather than a bypass of it.
//!
//! Being a transport, it never calls `Origin::local_front` — claiming local
//! trust on behalf of a peer is exactly the mistake that constructor is named to
//! make visible.

use std::sync::Arc;

use async_trait::async_trait;
use kamiroh_domain::{ControlMessage, ControlReply, EndpointId, PeerAddress};
use kamiroh_ports::{
    ControlApi, ControlApiError, ControllerError, Origin, Transport, TransportError,
};

/// A transport whose only reachable peer is the local node.
pub struct LoopbackTransport {
    local: EndpointId,
    api: Arc<dyn ControlApi>,
}

impl LoopbackTransport {
    /// Creates a loopback transport for a node with id `local`.
    pub fn new(local: EndpointId, api: Arc<dyn ControlApi>) -> Self {
        Self { local, api }
    }
}

#[async_trait]
impl Transport for LoopbackTransport {
    fn local_endpoint_id(&self) -> EndpointId {
        self.local
    }

    async fn send(
        &self,
        to: &PeerAddress,
        message: ControlMessage,
    ) -> Result<ControlReply, TransportError> {
        if to.endpoint != self.local {
            return Err(TransportError::Unreachable {
                endpoint: to.endpoint,
                detail: "loopback transport can only reach the local node".to_owned(),
            });
        }

        self.api
            .deliver(Origin::remote(self.local), &to.actor, message)
            .await
            .map_err(|error| match error {
                ControlApiError::NotAllowed { endpoint } => TransportError::Refused { endpoint },
                ControlApiError::Controller(ControllerError::NoSuchActor { actor }) => {
                    TransportError::NoSuchActor {
                        endpoint: self.local,
                        actor,
                    }
                }
                ControlApiError::Controller(other) => TransportError::Backend(Box::new(other)),
            })
    }
}

#[cfg(test)]
mod tests {
    use kamiroh_domain::{ActorName, AgentStatus};

    use super::*;

    /// A `ControlApi` that answers according to a fixed verdict.
    struct StubApi(Result<ControlReply, ()>);

    #[async_trait]
    impl ControlApi for StubApi {
        async fn deliver(
            &self,
            origin: Origin,
            _agent: &ActorName,
            _message: ControlMessage,
        ) -> Result<ControlReply, ControlApiError> {
            // A transport must always present the authenticated peer. `Origin`'s
            // constructors make claiming local trust a deliberate act, so this
            // is an assertion about the adapter, not a guard against the type.
            let endpoint = origin
                .remote_endpoint()
                .expect("a transport must present a remote origin");

            match &self.0 {
                Ok(reply) => Ok(reply.clone()),
                Err(()) => Err(ControlApiError::NotAllowed { endpoint }),
            }
        }
    }

    fn local() -> EndpointId {
        EndpointId::from_bytes([5u8; 32])
    }

    fn address(endpoint: EndpointId) -> PeerAddress {
        PeerAddress::new(endpoint, ActorName::new("agent").unwrap())
    }

    #[tokio::test]
    async fn sending_to_self_reaches_the_control_api() {
        let transport = LoopbackTransport::new(
            local(),
            Arc::new(StubApi(Ok(ControlReply::Status(AgentStatus::Idle)))),
        );

        let reply = transport
            .send(&address(local()), ControlMessage::Status)
            .await
            .unwrap();

        assert_eq!(reply, ControlReply::Status(AgentStatus::Idle));
    }

    #[tokio::test]
    async fn sending_to_another_node_is_unreachable() {
        let transport =
            LoopbackTransport::new(local(), Arc::new(StubApi(Ok(ControlReply::Accepted))));

        let error = transport
            .send(
                &address(EndpointId::from_bytes([6u8; 32])),
                ControlMessage::Status,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, TransportError::Unreachable { .. }));
    }

    #[tokio::test]
    async fn a_rejected_endpoint_surfaces_as_refused() {
        let transport = LoopbackTransport::new(local(), Arc::new(StubApi(Err(()))));

        let error = transport
            .send(&address(local()), ControlMessage::Status)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            TransportError::Refused { endpoint } if endpoint == local()
        ));
    }
}
