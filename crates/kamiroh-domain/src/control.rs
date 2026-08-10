//! The control vocabulary spoken to a controller actor.
//!
//! # Agent-agnostic payloads
//!
//! kamiroh makes no assumption about what an agent does, so the *verbs* here are
//! fixed (prompt / status / stop-waiting / detach) while the *content* is opaque:
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
    /// Stop waiting on the agent's current work.
    ///
    /// Named for what it does, which `Interrupt` was not: it abandons the run
    /// kamiroh is waiting on and answers whoever was waiting. **The agent
    /// carries on** — Herdr offers no way to interrupt one without per-kind
    /// keystrokes, which agent-agnostic forbids.
    ///
    /// Unlike [`Self::Detach`] the controller survives, so [`Self::Status`]
    /// still tells the truth about the agent afterwards, and a later prompt is
    /// accepted as normal.
    StopWaiting,
    /// Answer when the agent stops working, or when patience runs out.
    ///
    /// The verb for *"tell me when it needs me"*. A coding agent's work is long
    /// stretches of running punctuated by stops that need a human, and without
    /// this the only way to learn about one is to keep asking.
    ///
    /// Answered with [`ControlReply::Status`], which is why there is no new
    /// reply: [`Blocked`](AgentStatus::Blocked) means it needs a human,
    /// [`Idle`](AgentStatus::Idle) means it finished, and
    /// [`Busy`](AgentStatus::Busy) means patience ran out and the caller should
    /// ask again. Nothing is lost by asking again, because this reports *state*
    /// rather than delivering an event — a caller that was asleep, or on a
    /// train, learns the truth from its next question instead of having missed
    /// something.
    ///
    /// **It carries no timeout, deliberately.** How long a node holds an actor
    /// open is the node's business, not its caller's, and a peer that could name
    /// the number could name a large one.
    AwaitSettled,
    /// Stop controlling this agent.
    ///
    /// **The agent is not stopped** — kamiroh cannot stop one — it carries on,
    /// unwatched. Demonstrated rather than assumed: a live run answered this
    /// message at 17:22:01 and the agent wrote 297 lines of code at 17:27:32.
    ///
    /// Final for this node's lifetime: the controller is not re-created, so
    /// every later message to that name is refused as stopped. "Detach" names
    /// the relationship that ends, not one that can be resumed.
    Detach,
}

/// A controller actor's answer to a [`ControlMessage`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ControlReply {
    /// The message was accepted; no content is returned.
    Accepted,
    /// The agent's current state.
    Status(AgentStatus),
    /// Everything the agent produced; it is finished and ready for more.
    Output(Payload),
    /// What the agent has produced *so far*, and why there is not more of it.
    ///
    /// Two situations need this, and both are ordinary for a long-running
    /// agent rather than exceptional:
    ///
    /// - It is [`Blocked`](AgentStatus::Blocked) — waiting for a human to
    ///   answer something. The output holds the question.
    /// - It is still [`Busy`](AgentStatus::Busy) and the caller's patience ran
    ///   out first. Remote callers have a bounded wait; agents do not.
    ///
    /// [`Output`](Self::Output) would be a lie in both: it claims the agent is
    /// done. The distinction matters most where it is least visible — a caller
    /// on another node has no other way to learn that its agent is stuck.
    Partial {
        /// What the agent has produced so far.
        output: Payload,
        /// Why there is not more of it yet.
        status: AgentStatus,
    },
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
    /// Cannot proceed without a human: it has asked something and stopped.
    ///
    /// Agent-agnostic despite sounding specific. It says nothing about *what*
    /// was asked — only that the agent will not progress until someone
    /// answers. That is the single most useful thing to know about an agent
    /// you are not sitting next to.
    Blocked,
    /// No longer running.
    Stopped,
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Starting => "starting",
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Blocked => "blocked",
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
