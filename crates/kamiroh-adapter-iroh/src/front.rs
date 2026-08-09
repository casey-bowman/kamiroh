//! The inbound front: Iroh connections driving the [`ControlApi`].
//!
//! # The trust boundary
//!
//! Every inbound message's [`Origin`] is built from `connection.remote_id()` —
//! the endpoint Iroh **authenticated** during the TLS handshake — and never from
//! anything in the request frame, which a peer controls. This front never calls
//! `Origin::local_front()`; doing so would hand every remote peer the local
//! trust that bypasses the allowlist.
//!
//! # What an unauthorized peer can observe
//!
//! For any well-formed request, an unauthorized peer receives exactly one reply:
//! [`error_code::REFUSED`]. It is identical whichever agent is named and
//! whichever message is sent, because `ControlService` authorises before it
//! looks anything up — so no reply can betray which agents exist here.
//!
//! `NO_SUCH_ACTOR` is deliberately *not* folded into `REFUSED`: it can only
//! reach a peer that already passed the allowlist, and such a peer is trusted,
//! so an operator's typo deserves a real error rather than a misleading one.
//!
//! A malformed frame answers [`error_code::PROTOCOL`] to anybody. That reflects
//! only the sender's own bytes — it is the same answer regardless of whether the
//! sender is allowlisted — so it reveals nothing about this node.

use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Incoming;
use iroh::{Endpoint, EndpointId};
use kamiroh_ports::{ControlApi, ControlApiError, ControllerError, Origin};

use crate::codec::{self, MAX_FRAME_LEN, error_code};

/// How long a peer may take to send a complete request after opening a stream.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Serves inbound control connections until `endpoint` stops accepting.
///
/// Each connection is handled in its own task, so one slow agent cannot stall
/// the accept loop. Returns when the endpoint is closed.
pub async fn serve(endpoint: Endpoint, api: Arc<dyn ControlApi>) {
    while let Some(incoming) = endpoint.accept().await {
        let api = Arc::clone(&api);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(incoming, api).await {
                // A failed connection is routine — a peer hung up, sent
                // nonsense, or timed out. It must not stop the accept loop, but
                // it must not vanish silently either.
                tracing::debug!(%error, "inbound control connection failed");
            }
        });
    }
    tracing::info!("inbound control endpoint closed; no longer accepting peers");
}

/// Anything that can go wrong serving one connection.
#[derive(Debug, thiserror::Error)]
pub enum FrontError {
    /// The connection could not be established.
    #[error("connection failed: {0}")]
    Connect(String),
    /// A stream could not be read or written.
    #[error("stream failed: {0}")]
    Stream(String),
    /// The peer did not send a complete request in time.
    #[error("timed out reading a request from {peer}")]
    Timeout {
        /// The peer that went quiet.
        peer: EndpointId,
    },
}

async fn handle_connection(incoming: Incoming, api: Arc<dyn ControlApi>) -> Result<(), FrontError> {
    let connection = incoming
        .await
        .map_err(|error| FrontError::Connect(error.to_string()))?;

    // The authenticated peer. Not read from the request frame — that is peer
    // controlled, and trusting it would defeat the allowlist entirely.
    let peer = connection.remote_id();

    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|error| FrontError::Stream(error.to_string()))?;

    let request = tokio::time::timeout(REQUEST_TIMEOUT, recv.read_to_end(MAX_FRAME_LEN))
        .await
        .map_err(|_| FrontError::Timeout { peer })?
        .map_err(|error| FrontError::Stream(error.to_string()))?;

    let reply = handle_request(api.as_ref(), Origin::remote(to_domain(peer)), &request).await;

    send.write_all(&reply)
        .await
        .map_err(|error| FrontError::Stream(error.to_string()))?;
    send.finish()
        .map_err(|error| FrontError::Stream(error.to_string()))?;
    // Give the peer a moment to read before the connection drops.
    connection.closed().await;
    Ok(())
}

/// Converts an authenticated Iroh peer id into the domain's `EndpointId`.
fn to_domain(peer: EndpointId) -> kamiroh_domain::EndpointId {
    kamiroh_domain::EndpointId::from_bytes(*peer.as_bytes())
}

/// Decodes a request, delivers it, and encodes the reply.
///
/// Split out from the socket plumbing on purpose: this is where the security
/// properties live, and keeping it free of I/O means they can be tested without
/// binding a socket.
pub async fn handle_request(api: &dyn ControlApi, origin: Origin, request: &[u8]) -> Vec<u8> {
    let (agent, message) = match codec::decode_request(request) {
        Ok(decoded) => decoded,
        Err(error) => {
            tracing::debug!(%error, "undecodable control request");
            return codec::encode_error(error_code::PROTOCOL);
        }
    };

    match api.deliver(origin, &agent, message).await {
        Ok(reply) => codec::encode_reply(&reply),
        Err(error) => codec::encode_error(wire_code(&error)),
    }
}

/// Maps an application error to its wire code.
///
/// Numeric only: `Rejected` and `Backend` carry adapter-supplied text that has
/// no business crossing to a peer.
fn wire_code(error: &ControlApiError) -> u8 {
    match error {
        ControlApiError::NotAllowed { .. } => error_code::REFUSED,
        ControlApiError::Controller(ControllerError::NoSuchActor { .. }) => {
            error_code::NO_SUCH_ACTOR
        }
        ControlApiError::Controller(ControllerError::Stopped { .. }) => error_code::STOPPED,
        ControlApiError::Controller(ControllerError::Rejected { .. }) => error_code::REJECTED,
        ControlApiError::Controller(ControllerError::Timeout { .. }) => error_code::TIMEOUT,
        ControlApiError::Controller(ControllerError::Backend(_)) => error_code::INTERNAL,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kamiroh_adapter_memory::{EchoController, InMemoryAllowlist};
    use kamiroh_app::ControlService;
    use kamiroh_domain::{ActorName, ControlMessage, ControlReply, EndpointId, Payload};
    use kamiroh_ports::{AgentController, Allowlist};

    use super::*;
    use crate::codec::{DecodedReply, decode_reply, encode_request};

    fn endpoint(byte: u8) -> EndpointId {
        EndpointId::from_bytes([byte; 32])
    }

    fn agent() -> ActorName {
        ActorName::new("agent").unwrap()
    }

    /// A node allowing exactly `allowed`, hosting one agent called "agent".
    fn service(allowed: Vec<EndpointId>) -> ControlService {
        let allowlist: Arc<dyn Allowlist> = Arc::new(InMemoryAllowlist::with_endpoints(allowed));
        let controller: Arc<dyn AgentController> = Arc::new(EchoController::with_agents([agent()]));
        ControlService::new(allowlist, controller)
    }

    #[tokio::test]
    async fn an_allowlisted_peer_reaches_the_agent() {
        let service = service(vec![endpoint(1)]);
        let request = encode_request(&agent(), &ControlMessage::Prompt(Payload::text("hi")));

        let reply = handle_request(&service, Origin::remote(endpoint(1)), &request).await;

        assert_eq!(
            decode_reply(&reply).unwrap(),
            DecodedReply::Ok(ControlReply::Output(Payload::text("hi")))
        );
    }

    #[tokio::test]
    async fn an_unlisted_peer_learns_nothing_beyond_refused() {
        // The security property, stated as what an observer can distinguish:
        // every well-formed request from an unauthorized peer must produce
        // byte-identical output, whether or not the agent it names exists.
        let service = service(vec![endpoint(1)]);
        let stranger = Origin::remote(endpoint(99));

        let real = ActorName::new("agent").unwrap();
        let fake = ActorName::new("does-not-exist").unwrap();

        let mut replies = Vec::new();
        for (name, message) in [
            (&real, ControlMessage::Status),
            (&fake, ControlMessage::Status),
            (&real, ControlMessage::Shutdown),
            (&fake, ControlMessage::Interrupt),
            (&real, ControlMessage::Prompt(Payload::text("secret"))),
            (&fake, ControlMessage::Prompt(Payload::text("secret"))),
        ] {
            replies.push(handle_request(&service, stranger, &encode_request(name, &message)).await);
        }

        let first = &replies[0];
        assert_eq!(
            decode_reply(first).unwrap(),
            DecodedReply::Err(error_code::REFUSED)
        );
        for reply in &replies {
            assert_eq!(
                reply, first,
                "an unauthorized peer could distinguish two requests"
            );
        }
    }

    #[tokio::test]
    async fn an_admitted_peer_does_get_no_such_actor() {
        // The flip side: a trusted peer is told the truth, so a typo is
        // debuggable rather than reported as a permission problem.
        let service = service(vec![endpoint(1)]);
        let request = encode_request(&ActorName::new("typo").unwrap(), &ControlMessage::Status);

        let reply = handle_request(&service, Origin::remote(endpoint(1)), &request).await;

        assert_eq!(
            decode_reply(&reply).unwrap(),
            DecodedReply::Err(error_code::NO_SUCH_ACTOR)
        );
    }

    #[tokio::test]
    async fn a_malformed_frame_answers_the_same_way_to_anyone() {
        // PROTOCOL reflects the sender's own bytes, so it must not vary with
        // allowlist membership — otherwise it becomes a membership oracle.
        let service = service(vec![endpoint(1)]);
        let garbage = b"not a kamiroh frame";

        let admitted = handle_request(&service, Origin::remote(endpoint(1)), garbage).await;
        let stranger = handle_request(&service, Origin::remote(endpoint(99)), garbage).await;

        assert_eq!(
            decode_reply(&admitted).unwrap(),
            DecodedReply::Err(error_code::PROTOCOL)
        );
        assert_eq!(admitted, stranger);
    }

    #[tokio::test]
    async fn an_empty_allowlist_refuses_every_peer() {
        let service = service(vec![]);
        let request = encode_request(&agent(), &ControlMessage::Status);

        for byte in [0, 1, 42, 255] {
            let reply = handle_request(&service, Origin::remote(endpoint(byte)), &request).await;
            assert_eq!(
                decode_reply(&reply).unwrap(),
                DecodedReply::Err(error_code::REFUSED)
            );
        }
    }
}
