//! Outbound peer transport over Iroh.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use iroh::{Endpoint, EndpointAddr, PublicKey};
use kamiroh_domain::{ControlMessage, ControlReply, EndpointId, PeerAddress};
use kamiroh_ports::{Transport, TransportError};

use crate::codec::{self, DecodedReply, MAX_FRAME_LEN, error_code};

/// How long to wait for a connection to a peer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long to wait for a peer's reply once the request is sent.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Sends control messages to actors on remote nodes over Iroh.
///
/// # Addressing
///
/// The transport carries an explicit address book supplied by the composition
/// root, so a node reaches exactly the peers it has been configured with. That
/// matches kamiroh's posture — no central control gateway, peers known ahead of
/// time — and it is the only thing that works on a LAN or in a test.
///
/// Since M2 it can also dial a bare endpoint id, when the node's
/// [`Reach`](crate::Reach) says addresses can be looked up. That is what lets a
/// laptop reach a home node it has no route to: identity is enough, and the
/// address is resolved rather than known. The address book still wins where it
/// has an entry.
pub struct IrohTransport {
    endpoint: Endpoint,
    local: EndpointId,
    peers: HashMap<EndpointId, EndpointAddr>,
    reach: crate::Reach,
}

impl IrohTransport {
    /// Builds a transport over `endpoint` that can reach `peers`.
    pub fn new(
        endpoint: Endpoint,
        peers: impl IntoIterator<Item = EndpointAddr>,
        reach: crate::Reach,
    ) -> Self {
        let local = EndpointId::from_bytes(*endpoint.id().as_bytes());
        let peers = peers
            .into_iter()
            .map(|addr| (EndpointId::from_bytes(*addr.id.as_bytes()), addr))
            .collect();
        Self {
            endpoint,
            local,
            peers,
            reach,
        }
    }

    /// Where to dial a peer.
    ///
    /// A configured address wins: it is what the caller asked for, and it is
    /// the only thing that works on a LAN or in a test. Failing that, an
    /// id-only address hands the problem to Iroh's address lookup — which is
    /// the point of [`Reach::Anywhere`](crate::Reach::Anywhere), and is the
    /// difference between needing a peer's IP and needing only its identity.
    fn address_of(&self, endpoint: &EndpointId) -> Result<EndpointAddr, TransportError> {
        if let Some(address) = self.peers.get(endpoint) {
            return Ok(address.clone());
        }
        if self.reach.resolves_by_id() {
            return Ok(EndpointAddr::new(to_iroh_endpoint_id(endpoint)?));
        }
        Err(TransportError::Unreachable {
            endpoint: *endpoint,
            detail: "no address for this peer, and this node resolves nothing by id \
                     (set the reach to `anywhere` to look peers up)"
                .to_owned(),
        })
    }
}

#[async_trait]
impl Transport for IrohTransport {
    fn local_endpoint_id(&self) -> EndpointId {
        self.local
    }

    async fn send(
        &self,
        to: &PeerAddress,
        message: ControlMessage,
    ) -> Result<ControlReply, TransportError> {
        let address = self.address_of(&to.endpoint)?;
        let request = codec::encode_request(&to.actor, &message);

        let connection =
            tokio::time::timeout(CONNECT_TIMEOUT, self.endpoint.connect(address, crate::ALPN))
                .await
                .map_err(|_| TransportError::Timeout {
                    endpoint: to.endpoint,
                })?
                .map_err(|error| TransportError::Unreachable {
                    endpoint: to.endpoint,
                    detail: error.to_string(),
                })?;

        let (mut send, mut recv) = connection.open_bi().await.map_err(backend)?;
        send.write_all(&request).await.map_err(backend)?;
        send.finish().map_err(backend)?;

        let reply = tokio::time::timeout(REPLY_TIMEOUT, recv.read_to_end(MAX_FRAME_LEN))
            .await
            .map_err(|_| TransportError::Timeout {
                endpoint: to.endpoint,
            })?
            .map_err(backend)?;

        match codec::decode_reply(&reply)
            .map_err(|error| TransportError::Protocol(error.to_string()))?
        {
            DecodedReply::Ok(reply) => Ok(reply),
            DecodedReply::Err(code) => Err(from_wire(code, to)),
        }
    }
}

fn backend(error: impl core::error::Error + Send + Sync + 'static) -> TransportError {
    TransportError::Backend(Box::new(error))
}

/// Maps a peer's numeric failure code onto a local error.
///
/// The remote sends codes only; the descriptive text is produced here, on the
/// caller's side, where it is safe to be specific.
fn from_wire(code: u8, to: &PeerAddress) -> TransportError {
    match code {
        error_code::REFUSED => TransportError::Refused {
            endpoint: to.endpoint,
        },
        error_code::NO_SUCH_ACTOR => TransportError::NoSuchActor {
            endpoint: to.endpoint,
            actor: to.actor.to_string(),
        },
        error_code::TIMEOUT => TransportError::Timeout {
            endpoint: to.endpoint,
        },
        // The remote controller answered, but not with a reply. `TransportError`
        // has no variant for a controller outcome, and inventing one would push
        // controller semantics into the transport port; a described protocol
        // error keeps the boundary intact and still tells the caller what
        // happened.
        error_code::STOPPED => TransportError::Protocol(format!("agent {} has stopped", to.actor)),
        error_code::REJECTED => TransportError::Protocol(format!(
            "agent {} rejected the message in its current state",
            to.actor
        )),
        error_code::INTERNAL => {
            TransportError::Protocol("the peer reported an internal error".to_owned())
        }
        error_code::PROTOCOL => {
            TransportError::Protocol("the peer could not parse the request".to_owned())
        }
        unknown => TransportError::Protocol(format!("unknown error code {unknown} from peer")),
    }
}

/// Converts a domain endpoint id into Iroh's, for dialling.
///
/// Fails only if the bytes are not a valid ed25519 point — possible for an id
/// typed in by hand, impossible for one this node derived.
pub fn to_iroh_endpoint_id(endpoint: &EndpointId) -> Result<PublicKey, TransportError> {
    PublicKey::from_bytes(endpoint.as_bytes()).map_err(|error| {
        TransportError::Protocol(format!("{endpoint} is not a valid endpoint id: {error}"))
    })
}

/// Builds an address book entry for a peer reachable at `socket`.
///
/// Exists so the composition root can configure peers without depending on
/// `iroh` itself: constructing Iroh types is this adapter's job, and keeping it
/// here means the wiring names one crate rather than two.
pub fn peer_address(
    endpoint: &EndpointId,
    socket: SocketAddr,
) -> Result<EndpointAddr, TransportError> {
    Ok(EndpointAddr::new(to_iroh_endpoint_id(endpoint)?).with_ip_addr(socket))
}

#[cfg(test)]
mod tests {
    use kamiroh_domain::ActorName;

    use super::*;

    fn address(byte: u8) -> PeerAddress {
        PeerAddress::new(
            EndpointId::from_bytes([byte; 32]),
            ActorName::new("agent").unwrap(),
        )
    }

    #[test]
    fn refused_maps_to_refused_not_to_a_missing_actor() {
        let to = address(3);
        assert!(matches!(
            from_wire(error_code::REFUSED, &to),
            TransportError::Refused { endpoint } if endpoint == to.endpoint
        ));
    }

    #[test]
    fn no_such_actor_names_the_actor_locally() {
        let to = address(3);
        assert!(matches!(
            from_wire(error_code::NO_SUCH_ACTOR, &to),
            TransportError::NoSuchActor { actor, .. } if actor == "agent"
        ));
    }

    #[test]
    fn an_unknown_code_is_a_protocol_error_not_a_panic() {
        assert!(matches!(
            from_wire(200, &address(3)),
            TransportError::Protocol(_)
        ));
    }

    #[test]
    fn a_derived_endpoint_id_converts_back_to_iroh() {
        let secret = kamiroh_domain::NodeSecret::from_bytes([9u8; 32]);
        let id = crate::endpoint_id_for(&secret);
        assert_eq!(to_iroh_endpoint_id(&id).unwrap().as_bytes(), id.as_bytes());
    }
}
