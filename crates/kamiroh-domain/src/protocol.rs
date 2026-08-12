//! Protocols — named, legal sequences of vocabulary messages.
//!
//! Each party to a protocol is opaque: an agent or an embedding application,
//! on one side or both.

use crate::vocabulary::Message;

/// The protocols defined in v0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProtocolId {
    /// One [`Request`](crate::vocabulary::Request), one
    /// [`Ack`](crate::vocabulary::Ack). The first and simplest protocol.
    RequestAck,
    /// The lifecycle/test protocol (spawn / stop / ping). Privileged.
    Harness,
}

/// Which protocol a message belongs to.
pub fn protocol_of(message: &Message) -> ProtocolId {
    match message {
        Message::Request(_) | Message::Ack(_) => ProtocolId::RequestAck,
        Message::Harness(_) => ProtocolId::Harness,
    }
}
