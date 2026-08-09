//! The kamiroh control wire format.
//!
//! # Why a hand-written codec
//!
//! Deriving `serde` on the domain types would be less code, but it would give
//! `kamiroh-domain` a serde dependency — and the domain has had **zero**
//! dependencies since slice A. A wire format is an adapter concern; letting one
//! transport's serialization become a domain fact is backwards for a hexagonal
//! design, and every future transport would inherit it. The domain's public API
//! already exposes everything the wire needs, so the codec lives here.
//!
//! # Framing
//!
//! All integers are big-endian. Every length is bounded, and no length read off
//! the wire is ever used to allocate: [`Reader`] slices out of a buffer the
//! caller already capped at [`MAX_FRAME_LEN`], so an over-large declared length
//! fails a bounds check instead of reserving memory.
//!
//! Request:
//!
//! ```text
//! u8   version
//! u8   kind          1 Prompt · 2 Status · 3 Interrupt · 4 Detach
//! u8   actor_len     <= MAX_ACTOR_NAME_LEN
//! ..   actor         UTF-8
//! -- Prompt only:
//! u16  content_type_len
//! ..   content_type  UTF-8
//! u32  payload_len
//! ..   payload       opaque
//! ```
//!
//! Reply:
//!
//! ```text
//! u8   version
//! u8   kind          0 Error · 1 Accepted · 2 Status · 3 Output
//! -- Status only:  u8 status   1 Starting · 2 Idle · 3 Busy · 4 Stopped
//! -- Output only:  u16 content_type_len, .., u32 payload_len, ..
//! -- Error only:   u8 code     see `error_code`
//! ```
//!
//! Errors travel as numeric codes, never as text: `ControllerError::Rejected`
//! and `Backend` carry adapter-supplied strings, and shipping those to a peer
//! would leak internals. The rich text stays on the local side in
//! [`TransportError`](kamiroh_ports::TransportError).

use kamiroh_domain::actor::MAX_ACTOR_NAME_LEN;
use kamiroh_domain::{ActorName, AgentStatus, ControlMessage, ControlReply, Payload};

/// Wire format version. Bumped when framing changes incompatibly.
pub const PROTOCOL_VERSION: u8 = 1;

/// Largest frame accepted in either direction.
///
/// Load-bearing: it is the argument to every `read_to_end`, and therefore the
/// real bound on what a hostile peer can make this node buffer.
pub const MAX_FRAME_LEN: usize = 1024 * 1024;

mod request_kind {
    pub const PROMPT: u8 = 1;
    pub const STATUS: u8 = 2;
    pub const INTERRUPT: u8 = 3;
    pub const DETACH: u8 = 4;
}

mod reply_kind {
    pub const ERROR: u8 = 0;
    pub const ACCEPTED: u8 = 1;
    pub const STATUS: u8 = 2;
    pub const OUTPUT: u8 = 3;
    pub const PARTIAL: u8 = 4;
}

/// Wire values for [`AgentStatus`]. Written out rather than derived, so that
/// reordering the enum cannot silently renumber the protocol.
mod status_code {
    pub const STARTING: u8 = 1;
    pub const IDLE: u8 = 2;
    pub const BUSY: u8 = 3;
    pub const STOPPED: u8 = 4;
    pub const BLOCKED: u8 = 5;
}

/// Numeric reply codes for failures.
pub mod error_code {
    /// The peer is not on this node's allowlist.
    pub const REFUSED: u8 = 1;
    /// No agent by that name. Only ever sent to an admitted peer.
    pub const NO_SUCH_ACTOR: u8 = 2;
    /// The agent exists but has stopped.
    pub const STOPPED: u8 = 3;
    /// The agent refused the message in its current state.
    pub const REJECTED: u8 = 4;
    /// The agent did not reply in time.
    pub const TIMEOUT: u8 = 5;
    /// Anything else. Deliberately opaque.
    pub const INTERNAL: u8 = 6;
    /// The request could not be parsed.
    pub const PROTOCOL: u8 = 7;
}

/// Why a frame could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// The frame ended before the value being read.
    #[error("frame truncated: wanted {wanted} more bytes, {available} available")]
    Truncated {
        /// Bytes the field needed.
        wanted: usize,
        /// Bytes actually left.
        available: usize,
    },
    /// The version byte was not one this build speaks.
    #[error("unsupported protocol version {got}, expected {PROTOCOL_VERSION}")]
    Version {
        /// The version the peer announced.
        got: u8,
    },
    /// The kind or code byte was not a known discriminant.
    #[error("unknown {field} discriminant {got}")]
    Discriminant {
        /// Which field was unrecognised.
        field: &'static str,
        /// The byte received.
        got: u8,
    },
    /// A length field exceeded what the protocol allows.
    #[error("{field} length {got} exceeds the maximum {max}")]
    TooLong {
        /// Which field was over-long.
        field: &'static str,
        /// The declared length.
        got: usize,
        /// The permitted maximum.
        max: usize,
    },
    /// A field that must be UTF-8 was not.
    #[error("{field} is not valid UTF-8")]
    NotUtf8 {
        /// Which field was malformed.
        field: &'static str,
    },
    /// Trailing bytes after a complete frame.
    #[error("{count} unexpected trailing bytes")]
    Trailing {
        /// How many bytes were left over.
        count: usize,
    },
}

/// A bounds-checked cursor over a received frame.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CodecError> {
        if self.bytes.len() < count {
            return Err(CodecError::Truncated {
                wanted: count,
                available: self.bytes.len(),
            });
        }
        let (head, rest) = self.bytes.split_at(count);
        self.bytes = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<usize, CodecError> {
        let bytes = self.take(2)?;
        Ok(usize::from(u16::from_be_bytes([bytes[0], bytes[1]])))
    }

    fn u32(&mut self) -> Result<usize, CodecError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
    }

    /// Reads a length-prefixed slice, rejecting anything over `max`.
    ///
    /// The `max` check is belt-and-braces: `take` already refuses to read past
    /// the end of the frame, so an inflated length cannot cause an allocation.
    fn bounded(
        &mut self,
        len: usize,
        field: &'static str,
        max: usize,
    ) -> Result<&'a [u8], CodecError> {
        if len > max {
            return Err(CodecError::TooLong {
                field,
                got: len,
                max,
            });
        }
        self.take(len)
    }

    fn finish(self) -> Result<(), CodecError> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(CodecError::Trailing {
                count: self.bytes.len(),
            })
        }
    }
}

fn version(reader: &mut Reader<'_>) -> Result<(), CodecError> {
    match reader.u8()? {
        PROTOCOL_VERSION => Ok(()),
        got => Err(CodecError::Version { got }),
    }
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8], len_prefix: LenPrefix) {
    match len_prefix {
        LenPrefix::U8 => out.push(bytes.len() as u8),
        LenPrefix::U16 => out.extend_from_slice(&(bytes.len() as u16).to_be_bytes()),
        LenPrefix::U32 => out.extend_from_slice(&(bytes.len() as u32).to_be_bytes()),
    }
    out.extend_from_slice(bytes);
}

#[derive(Clone, Copy)]
enum LenPrefix {
    U8,
    U16,
    U32,
}

fn put_payload(out: &mut Vec<u8>, payload: &Payload) {
    put_bytes(out, payload.content_type().as_bytes(), LenPrefix::U16);
    put_bytes(out, payload.bytes(), LenPrefix::U32);
}

fn read_payload(reader: &mut Reader<'_>) -> Result<Payload, CodecError> {
    let len = reader.u16()?;
    let content_type = reader.bounded(len, "content_type", MAX_FRAME_LEN)?;
    let content_type = core::str::from_utf8(content_type).map_err(|_| CodecError::NotUtf8 {
        field: "content_type",
    })?;

    let len = reader.u32()?;
    let bytes = reader.bounded(len, "payload", MAX_FRAME_LEN)?;

    Ok(Payload::new(content_type, bytes))
}

/// Encodes a request for `agent`.
pub fn encode_request(agent: &ActorName, message: &ControlMessage) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(PROTOCOL_VERSION);
    out.push(match message {
        ControlMessage::Prompt(_) => request_kind::PROMPT,
        ControlMessage::Status => request_kind::STATUS,
        ControlMessage::Interrupt => request_kind::INTERRUPT,
        ControlMessage::Detach => request_kind::DETACH,
    });
    // `ActorName` is capped at 64 bytes by the domain, so a u8 length is safe.
    put_bytes(&mut out, agent.as_str().as_bytes(), LenPrefix::U8);
    if let ControlMessage::Prompt(payload) = message {
        put_payload(&mut out, payload);
    }
    out
}

/// Decodes a request into the agent it targets and the message for it.
pub fn decode_request(bytes: &[u8]) -> Result<(ActorName, ControlMessage), CodecError> {
    let mut reader = Reader::new(bytes);
    version(&mut reader)?;
    let kind = reader.u8()?;

    let len = reader.u8()? as usize;
    // Rejected here rather than by `ActorName::new` so an over-long name is a
    // protocol error, which is what it is.
    let actor = reader.bounded(len, "actor", MAX_ACTOR_NAME_LEN)?;
    let actor = core::str::from_utf8(actor).map_err(|_| CodecError::NotUtf8 { field: "actor" })?;
    let actor = ActorName::new(actor).map_err(|_| CodecError::NotUtf8 { field: "actor" })?;

    let message = match kind {
        request_kind::PROMPT => ControlMessage::Prompt(read_payload(&mut reader)?),
        request_kind::STATUS => ControlMessage::Status,
        request_kind::INTERRUPT => ControlMessage::Interrupt,
        request_kind::DETACH => ControlMessage::Detach,
        got => {
            return Err(CodecError::Discriminant {
                field: "request kind",
                got,
            });
        }
    };

    reader.finish()?;
    Ok((actor, message))
}

/// Encodes a successful reply.
pub fn encode_reply(reply: &ControlReply) -> Vec<u8> {
    let mut out = vec![PROTOCOL_VERSION];
    match reply {
        ControlReply::Accepted => out.push(reply_kind::ACCEPTED),
        ControlReply::Status(status) => {
            out.push(reply_kind::STATUS);
            out.push(status_byte(*status));
        }
        ControlReply::Output(payload) => {
            out.push(reply_kind::OUTPUT);
            put_payload(&mut out, payload);
        }
        ControlReply::Partial { output, status } => {
            out.push(reply_kind::PARTIAL);
            // Fixed-width field first, then the variable one.
            out.push(status_byte(*status));
            put_payload(&mut out, output);
        }
    }
    out
}

/// The wire byte for a status.
fn status_byte(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Starting => status_code::STARTING,
        AgentStatus::Idle => status_code::IDLE,
        AgentStatus::Busy => status_code::BUSY,
        AgentStatus::Stopped => status_code::STOPPED,
        AgentStatus::Blocked => status_code::BLOCKED,
    }
}

/// Reads a status byte, rejecting one this build does not know.
///
/// A peer built before `Blocked` existed answers `Discriminant` here rather
/// than guessing — which is why adding a status needed no version bump.
fn read_status(reader: &mut Reader<'_>) -> Result<AgentStatus, CodecError> {
    match reader.u8()? {
        status_code::STARTING => Ok(AgentStatus::Starting),
        status_code::IDLE => Ok(AgentStatus::Idle),
        status_code::BUSY => Ok(AgentStatus::Busy),
        status_code::STOPPED => Ok(AgentStatus::Stopped),
        status_code::BLOCKED => Ok(AgentStatus::Blocked),
        got => Err(CodecError::Discriminant {
            field: "agent status",
            got,
        }),
    }
}

/// Encodes a failure reply as an opaque numeric code.
pub fn encode_error(code: u8) -> Vec<u8> {
    vec![PROTOCOL_VERSION, reply_kind::ERROR, code]
}

/// A decoded reply: either a control reply, or a failure code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedReply {
    /// The call succeeded.
    Ok(ControlReply),
    /// The call failed with this wire code.
    Err(u8),
}

/// Decodes a reply.
pub fn decode_reply(bytes: &[u8]) -> Result<DecodedReply, CodecError> {
    let mut reader = Reader::new(bytes);
    version(&mut reader)?;

    let decoded = match reader.u8()? {
        reply_kind::ACCEPTED => DecodedReply::Ok(ControlReply::Accepted),
        reply_kind::STATUS => DecodedReply::Ok(ControlReply::Status(read_status(&mut reader)?)),
        reply_kind::OUTPUT => DecodedReply::Ok(ControlReply::Output(read_payload(&mut reader)?)),
        reply_kind::PARTIAL => {
            let status = read_status(&mut reader)?;
            DecodedReply::Ok(ControlReply::Partial {
                output: read_payload(&mut reader)?,
                status,
            })
        }
        reply_kind::ERROR => DecodedReply::Err(reader.u8()?),
        got => {
            return Err(CodecError::Discriminant {
                field: "reply kind",
                got,
            });
        }
    };

    reader.finish()?;
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> ActorName {
        ActorName::new("agent").unwrap()
    }

    #[test]
    fn every_request_kind_round_trips() {
        let messages = [
            ControlMessage::Status,
            ControlMessage::Interrupt,
            ControlMessage::Detach,
            ControlMessage::Prompt(Payload::text("build the thing")),
        ];
        for message in messages {
            let encoded = encode_request(&agent(), &message);
            let (actor, decoded) = decode_request(&encoded).unwrap();
            assert_eq!(actor, agent());
            assert_eq!(decoded, message, "round trip failed for {message:?}");
        }
    }

    #[test]
    fn every_reply_kind_round_trips() {
        let replies = [
            ControlReply::Accepted,
            ControlReply::Status(AgentStatus::Starting),
            ControlReply::Status(AgentStatus::Idle),
            ControlReply::Status(AgentStatus::Busy),
            ControlReply::Status(AgentStatus::Stopped),
            ControlReply::Output(Payload::text("done")),
        ];
        for reply in replies {
            let encoded = encode_reply(&reply);
            assert_eq!(decode_reply(&encoded).unwrap(), DecodedReply::Ok(reply));
        }
    }

    #[test]
    fn binary_payloads_survive_intact() {
        // Agent-agnostic means the wire must not assume text.
        let payload = Payload::new("application/octet-stream", vec![0x00, 0xff, 0x80, 0x0a]);
        let encoded = encode_request(&agent(), &ControlMessage::Prompt(payload.clone()));
        let (_, decoded) = decode_request(&encoded).unwrap();
        assert_eq!(decoded, ControlMessage::Prompt(payload));
    }

    #[test]
    fn error_codes_round_trip() {
        for code in [
            error_code::REFUSED,
            error_code::NO_SUCH_ACTOR,
            error_code::INTERNAL,
        ] {
            assert_eq!(
                decode_reply(&encode_error(code)).unwrap(),
                DecodedReply::Err(code)
            );
        }
    }

    #[test]
    fn an_empty_frame_is_truncated_not_a_panic() {
        assert!(matches!(
            decode_request(&[]),
            Err(CodecError::Truncated { .. })
        ));
        assert!(matches!(
            decode_reply(&[]),
            Err(CodecError::Truncated { .. })
        ));
    }

    #[test]
    fn a_wrong_version_is_rejected() {
        let mut frame = encode_request(&agent(), &ControlMessage::Status);
        frame[0] = 99;
        assert_eq!(decode_request(&frame), Err(CodecError::Version { got: 99 }));
    }

    #[test]
    fn unknown_discriminants_are_rejected() {
        let mut frame = encode_request(&agent(), &ControlMessage::Status);
        frame[1] = 77;
        assert!(matches!(
            decode_request(&frame),
            Err(CodecError::Discriminant { .. })
        ));
    }

    #[test]
    fn a_lying_length_cannot_cause_an_allocation() {
        // The hostile case: a peer declares a huge payload. Decoding must fail
        // on the frame's own bounds, never reserve the declared size.
        let mut frame = encode_request(&agent(), &ControlMessage::Prompt(Payload::text("x")));
        let len = frame.len();
        // Overwrite the u32 payload length with u32::MAX.
        frame[len - 5..len - 1].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            decode_request(&frame),
            Err(CodecError::Truncated { .. } | CodecError::TooLong { .. })
        ));
    }

    #[test]
    fn an_over_long_actor_name_is_a_protocol_error() {
        // Hand-built: the encoder cannot produce this, since `ActorName` caps at
        // 64 bytes. A hostile peer is not so constrained.
        let mut frame = vec![PROTOCOL_VERSION, request_kind::STATUS, 200];
        frame.extend(core::iter::repeat_n(b'a', 200));
        assert!(matches!(
            decode_request(&frame),
            Err(CodecError::TooLong { field: "actor", .. })
        ));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut frame = encode_request(&agent(), &ControlMessage::Status);
        frame.push(0);
        assert!(matches!(
            decode_request(&frame),
            Err(CodecError::Trailing { count: 1 })
        ));
    }

    #[test]
    fn a_non_utf8_actor_name_is_rejected() {
        let mut frame = vec![PROTOCOL_VERSION, request_kind::STATUS, 2];
        frame.extend_from_slice(&[0xff, 0xfe]);
        assert!(matches!(
            decode_request(&frame),
            Err(CodecError::NotUtf8 { field: "actor" })
        ));
    }
}
