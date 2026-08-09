//! The control vocabulary spoken to a controller actor.
//!
//! # Agent-agnostic payloads
//!
//! kamiroh makes no assumption about what an agent does, so the *verbs* here are
//! fixed (prompt / status / interrupt / shutdown) while the *content* is opaque:
//! a [`Payload`] is bytes plus a content type, and only the agent behind the
//! controller interprets it. `Payload::text` exists as a convenience for the
//! common text-in/text-out case, not as a statement that agents are textual.

use core::fmt;

/// An opaque, typed blob handed to or returned by an agent.
#[derive(Clone, PartialEq, Eq)]
pub struct Payload {
    /// A media type describing `bytes`, e.g. `text/plain; charset=utf-8`.
    content_type: String,
    /// The uninterpreted content.
    bytes: Vec<u8>,
}

/// Content type used by [`Payload::text`].
pub const TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

impl Payload {
    /// Builds a payload from a content type and raw bytes.
    pub fn new(content_type: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            content_type: content_type.into(),
            bytes: bytes.into(),
        }
    }

    /// Builds a UTF-8 text payload.
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(TEXT_CONTENT_TYPE, text.into().into_bytes())
    }

    /// The declared content type.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// The raw content.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Interprets the content as UTF-8, if it is valid UTF-8.
    pub fn as_text(&self) -> Option<&str> {
        core::str::from_utf8(&self.bytes).ok()
    }
}

/// Summarises rather than dumping content: payloads can be large or binary.
impl fmt::Debug for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Payload")
            .field("content_type", &self.content_type)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// A message driving one agent's controller actor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ControlMessage {
    /// Give the agent work to do.
    Prompt(Payload),
    /// Ask what the agent is currently doing.
    Status,
    /// Ask the agent to abandon its current work but stay alive.
    Interrupt,
    /// Ask the agent to stop.
    Shutdown,
}

/// A controller actor's answer to a [`ControlMessage`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ControlReply {
    /// The message was accepted; no content is returned.
    Accepted,
    /// The agent's current state.
    Status(AgentStatus),
    /// Content produced by the agent.
    Output(Payload),
}

/// Coarse lifecycle state of an agent, as seen by its controller.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgentStatus {
    /// Controller exists; the agent is not yet ready for work.
    Starting,
    /// Ready and not currently working.
    Idle,
    /// Working on a prompt.
    Busy,
    /// No longer running.
    Stopped,
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Starting => "starting",
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Stopped => "stopped",
        };
        f.write_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_payload_round_trips() {
        let payload = Payload::text("build the thing");
        assert_eq!(payload.as_text(), Some("build the thing"));
        assert_eq!(payload.content_type(), TEXT_CONTENT_TYPE);
    }

    #[test]
    fn binary_payload_stays_opaque() {
        let payload = Payload::new("application/octet-stream", vec![0xff, 0x00, 0xfe]);
        assert_eq!(payload.bytes(), &[0xff, 0x00, 0xfe]);
        assert_eq!(payload.as_text(), None);
    }

    #[test]
    fn debug_reports_length_not_content() {
        let rendered = format!("{:?}", Payload::text("hunter2"));
        assert!(rendered.contains("len: 7"), "{rendered}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
    }
}
