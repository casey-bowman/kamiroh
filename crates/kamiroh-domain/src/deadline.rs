//! Deadlines — how long each side is willing to wait, as pure data.
//!
//! Time itself never enters the domain (`ARCHITECTURE.md`, decision 24): a
//! [`Deadlines`] value only *names* durations, and the domain only ever
//! learns that one *elapsed* — a verdict fed in from outside, produced by a
//! runtime's timer.
//!
//! Deadlines are **finite and mandatory** (decision 22): there is no
//! `Default`, no `Option`, and no unbounded variant. Every conversation
//! surface that waits requires one at construction, so "this exchange can
//! hang forever" is unrepresentable. Each side configures its own patience
//! and applies it to its own waiting only; nothing about deadlines crosses
//! the wire, so neither party knows how long the other will wait.

use std::time::Duration;

/// How long one side waits, per kind of wait, in one conversation.
///
/// The two kinds match the two waits the protocol has (decision 4 made them
/// distinct): the delivery receipt, and the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadlines {
    /// Patience for the delivery ack of a turn's request half —
    /// transport-scale, typically short.
    pub ack: Duration,
    /// Patience for the peer's next turn while it thinks — party-scale,
    /// possibly long (an agent may think for minutes).
    pub turn: Duration,
}

impl Deadlines {
    pub fn new(ack: Duration, turn: Duration) -> Self {
        Self { ack, turn }
    }
}

/// Which wait a deadline bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineKind {
    /// Waiting for the delivery ack of a sent request half.
    Ack,
    /// Waiting for the peer's next turn.
    Turn,
}

/// Why an exchange failed. Local knowledge only — no failure message ever
/// crosses the wire (decision 22): each side reaches its own verdict on its
/// own evidence, and the two sides may fail the same exchange at different
/// moments (or one may never learn at all until its own deadline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCause {
    /// A deadline elapsed in silence.
    DeadlineElapsed(DeadlineKind),
    /// The transport refused to carry a turn of this exchange — positive
    /// local evidence that it never left, so the exchange fails at once
    /// rather than waiting out a deadline the transport already answered
    /// (decision 26).
    SendFailed,
    /// The local party replied with an illegal turn (wrong response id,
    /// out-of-turn kind). The reply was dropped; a silently un-deadlined
    /// exchange would be a hang, so it fails loudly instead (decision 26).
    IllegalReply,
    /// The transport reported the peer's endpoint gone — a connection
    /// closed by the peer, timed out, or reset (decision 27). Positive
    /// evidence, so live exchanges fail at once; the conversation itself
    /// survives, as it must (a conversation spans connections).
    Disconnected,
    /// This side's own operator revoked the peer's endpoint (decision 28)
    /// — positive local evidence that nothing further will be heard from
    /// it, so live exchanges fail at once. The conversation survives and a
    /// fresh exchange is legal on re-admission — but unlike
    /// [`FailureCause::Disconnected`], reopening *unprompted* is wrong:
    /// the peer was cut off deliberately, and its replies will be denied.
    Revoked,
}

impl std::fmt::Display for FailureCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailureCause::DeadlineElapsed(DeadlineKind::Ack) => {
                f.write_str("ack deadline elapsed: delivery was never confirmed")
            }
            FailureCause::DeadlineElapsed(DeadlineKind::Turn) => {
                f.write_str("turn deadline elapsed: the peer's turn never arrived")
            }
            FailureCause::SendFailed => {
                f.write_str("the transport refused to carry a turn of this exchange")
            }
            FailureCause::IllegalReply => {
                f.write_str("the local party replied with an illegal turn")
            }
            FailureCause::Disconnected => {
                f.write_str("the transport reported the peer's endpoint gone")
            }
            FailureCause::Revoked => f.write_str("this side revoked the peer's endpoint"),
        }
    }
}
