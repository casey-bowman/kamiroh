//! Two real Iroh nodes over localhost.
//!
//! The unit tests in `front` and `codec` cover the protocol without sockets;
//! these cover what only a live connection can: that the two halves negotiate,
//! that the peer id the front authorises with is the one the dialer actually
//! holds the key for, and that the allowlist governs a real connection.
//!
//! **These tests need permission to bind UDP sockets.** They pass on an ordinary
//! machine and fail with `Operation not permitted` inside a restrictive sandbox;
//! that is an environment limit, not a project one.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use iroh::{Endpoint, EndpointAddr};
use kamiroh_adapter_iroh::{IrohTransport, bind_endpoint, endpoint_id_for, front};
use kamiroh_adapter_memory::{EchoController, InMemoryAllowlist};
use kamiroh_app::ControlService;
use kamiroh_domain::{
    ActorName, ControlMessage, ControlReply, EndpointId, NodeSecret, Payload, PeerAddress,
};
use kamiroh_ports::{AgentController, Allowlist, ControlApi, Transport, TransportError};

fn agent() -> ActorName {
    ActorName::new("agent").unwrap()
}

/// A dialable address for a locally bound endpoint.
///
/// `bound_sockets` reports wildcard addresses like `0.0.0.0:PORT`; a dialer needs
/// a concrete one, and these nodes are in the same process.
fn local_addr(endpoint: &Endpoint) -> EndpointAddr {
    let port = endpoint
        .bound_sockets()
        .into_iter()
        .find(|addr| addr.is_ipv4())
        .expect("endpoint bound no IPv4 socket")
        .port();
    EndpointAddr::new(endpoint.id())
        .with_ip_addr(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

/// Starts a server node hosting one agent and allowing `allowed`.
async fn serve_node(secret: &NodeSecret, allowed: Vec<EndpointId>) -> (EndpointAddr, EndpointId) {
    let endpoint = bind_endpoint(secret).await.expect("bind server");
    let addr = local_addr(&endpoint);
    let id = endpoint_id_for(secret);

    let allowlist: Arc<dyn Allowlist> = Arc::new(InMemoryAllowlist::with_endpoints(allowed));
    let controller: Arc<dyn AgentController> = Arc::new(EchoController::with_agents([agent()]));
    let api: Arc<dyn ControlApi> = Arc::new(ControlService::new(allowlist, controller));

    tokio::spawn(front::serve(endpoint, api));
    (addr, id)
}

/// Builds a client transport for `secret` that knows how to reach `peer`.
async fn client(secret: &NodeSecret, peer: EndpointAddr) -> IrohTransport {
    let endpoint = bind_endpoint(secret).await.expect("bind client");
    IrohTransport::new(endpoint, [peer])
}

#[tokio::test]
async fn an_allowlisted_peer_drives_an_agent_on_another_node() {
    let server_secret = NodeSecret::from_bytes([11u8; 32]);
    let client_secret = NodeSecret::from_bytes([22u8; 32]);
    let client_id = endpoint_id_for(&client_secret);

    // The server admits exactly this client.
    let (server_addr, server_id) = serve_node(&server_secret, vec![client_id]).await;
    let transport = client(&client_secret, server_addr).await;

    let reply = transport
        .send(
            &PeerAddress::new(server_id, agent()),
            ControlMessage::Prompt(Payload::text("hello across the wire")),
        )
        .await
        .expect("send should succeed");

    assert_eq!(
        reply,
        ControlReply::Output(Payload::text("hello across the wire"))
    );
}

#[tokio::test]
async fn an_unlisted_peer_is_refused_over_a_real_connection() {
    let server_secret = NodeSecret::from_bytes([33u8; 32]);
    let client_secret = NodeSecret::from_bytes([44u8; 32]);

    // Empty allowlist: this client is a stranger even though it can connect.
    let (server_addr, server_id) = serve_node(&server_secret, vec![]).await;
    let transport = client(&client_secret, server_addr).await;

    let error = transport
        .send(
            &PeerAddress::new(server_id, agent()),
            ControlMessage::Status,
        )
        .await
        .expect_err("an unlisted peer must be refused");

    assert!(
        matches!(error, TransportError::Refused { endpoint } if endpoint == server_id),
        "expected Refused, got {error:?}"
    );
}

#[tokio::test]
async fn a_refused_peer_cannot_tell_a_real_agent_from_a_missing_one() {
    // The security property over a live connection: identity of the reply, not
    // merely that both fail.
    let server_secret = NodeSecret::from_bytes([55u8; 32]);
    let client_secret = NodeSecret::from_bytes([66u8; 32]);

    let (server_addr, server_id) = serve_node(&server_secret, vec![]).await;
    let transport = client(&client_secret, server_addr).await;

    let real = transport
        .send(
            &PeerAddress::new(server_id, agent()),
            ControlMessage::Status,
        )
        .await
        .expect_err("refused");
    let fake = transport
        .send(
            &PeerAddress::new(server_id, ActorName::new("no-such-agent").unwrap()),
            ControlMessage::Status,
        )
        .await
        .expect_err("refused");

    assert!(matches!(real, TransportError::Refused { .. }));
    assert!(
        matches!(fake, TransportError::Refused { .. }),
        "naming a missing agent revealed it does not exist: {fake:?}"
    );
}

#[tokio::test]
async fn an_admitted_peer_is_told_when_an_agent_is_missing() {
    let server_secret = NodeSecret::from_bytes([77u8; 32]);
    let client_secret = NodeSecret::from_bytes([88u8; 32]);
    let client_id = endpoint_id_for(&client_secret);

    let (server_addr, server_id) = serve_node(&server_secret, vec![client_id]).await;
    let transport = client(&client_secret, server_addr).await;

    let error = transport
        .send(
            &PeerAddress::new(server_id, ActorName::new("typo").unwrap()),
            ControlMessage::Status,
        )
        .await
        .expect_err("no such actor");

    assert!(
        matches!(error, TransportError::NoSuchActor { ref actor, .. } if actor == "typo"),
        "expected NoSuchActor, got {error:?}"
    );
}

#[tokio::test]
async fn an_unconfigured_peer_is_unreachable_without_dialling() {
    let secret = NodeSecret::from_bytes([99u8; 32]);
    let endpoint = bind_endpoint(&secret).await.expect("bind");
    let transport = IrohTransport::new(endpoint, []);

    let unknown = EndpointId::from_bytes([0xab; 32]);
    let error = transport
        .send(&PeerAddress::new(unknown, agent()), ControlMessage::Status)
        .await
        .expect_err("no address configured");

    assert!(matches!(error, TransportError::Unreachable { .. }));
}

#[tokio::test]
async fn the_local_endpoint_id_matches_the_derived_one() {
    // Ties the transport back to key custody: the id this node reports is the
    // one derived from its secret, so peers can be configured from it.
    let secret = NodeSecret::from_bytes([123u8; 32]);
    let endpoint = bind_endpoint(&secret).await.expect("bind");
    let transport = IrohTransport::new(endpoint, []);

    assert_eq!(transport.local_endpoint_id(), endpoint_id_for(&secret));
}
