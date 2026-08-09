//! The one agent a pane is bound to, and how messages reach it.
//!
//! A pane drives exactly one agent. Whether that agent runs on this node or on
//! another is a property of the *link*, not of the console — so the console
//! never branches on it, and typing at a remote agent feels no different from
//! typing at a local one. That is the whole point: [`PeerAddress`] already
//! carries "which node, which actor", and a pane bound to one of those is the
//! human-sized version of the same idea.
//!
//! The two implementations reach for opposite ports, which is worth naming
//! because the distinction is easy to lose:
//!
//! | | port | direction | trust |
//! |---|---|---|---|
//! | [`LocalLink`] | `ControlApi` (driving) | inbound — a *front* | `Origin::local_front()` |
//! | [`RemoteLink`] | `Transport` (driven) | outbound — a *console* | the peer's allowlist decides |
//!
//! `kamiroh-adapter-iroh` is a front in the first sense and a console in the
//! second. This crate is the same pair, with a terminal on the near end.

use std::sync::Arc;

use async_trait::async_trait;
use kamiroh_domain::{ActorName, ControlMessage, ControlReply, PeerAddress};
use kamiroh_ports::{ControlApi, ControlApiError, Origin, Transport, TransportError};

/// A one-way path to the single agent a pane drives.
#[async_trait]
pub trait Link: Send + Sync + 'static {
    /// Sends `message` to the agent and waits for its reply.
    async fn send(&self, message: ControlMessage) -> Result<ControlReply, LinkError>;

    /// How to describe this agent to the person at the pane.
    fn describe(&self) -> String;
}

/// An agent on this node, reached through the driving port.
///
/// This is the case the architecture calls "a second front": it holds the same
/// `Arc<dyn ControlApi>` as the Iroh front, so both reach one controller actor.
pub struct LocalLink {
    api: Arc<dyn ControlApi>,
    agent: ActorName,
}

impl LocalLink {
    /// Binds a pane to `agent` on this node.
    pub fn new(api: Arc<dyn ControlApi>, agent: ActorName) -> Self {
        Self { api, agent }
    }
}

#[async_trait]
impl Link for LocalLink {
    async fn send(&self, message: ControlMessage) -> Result<ControlReply, LinkError> {
        // The one place in this crate that claims local trust. A pane is a
        // process on this machine, started by whoever owns it, so it sits
        // inside the boundary the allowlist defends — the same position
        // `Origin::local_front()` was added for in slice B.
        Ok(self
            .api
            .deliver(Origin::local_front(), &self.agent, message)
            .await?)
    }

    fn describe(&self) -> String {
        format!("{} on this node", self.agent)
    }
}

/// An agent on another node, reached through the transport.
///
/// Not a front: nothing arrives here. This is kamiroh being a *client* of a
/// peer, which is the direction a person at a pane actually wants — the agents
/// worth driving are the long-running ones on the home node, and the pane is
/// wherever you happen to be sitting.
pub struct RemoteLink {
    transport: Arc<dyn Transport>,
    address: PeerAddress,
}

impl RemoteLink {
    /// Binds a pane to an agent on the node named by `address`.
    pub fn new(transport: Arc<dyn Transport>, address: PeerAddress) -> Self {
        Self { transport, address }
    }
}

#[async_trait]
impl Link for RemoteLink {
    async fn send(&self, message: ControlMessage) -> Result<ControlReply, LinkError> {
        Ok(self.transport.send(&self.address, message).await?)
    }

    fn describe(&self) -> String {
        format!("{} on peer {}", self.address.actor, self.address.endpoint)
    }
}

/// Why a message from the pane did not produce a reply.
///
/// The two halves stay distinct rather than collapsing to a string: a refusal
/// from a peer's allowlist and a missing actor on this node are different
/// problems with different fixes, and the person at the pane is the one who has
/// to tell them apart.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// The local control API refused or failed.
    #[error(transparent)]
    Local(#[from] ControlApiError),

    /// The peer could not be reached, refused us, or had no such actor.
    #[error(transparent)]
    Remote(#[from] TransportError),
}
