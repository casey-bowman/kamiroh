//! Where this process sits in a Herdr session, and how to talk to it.
//!
//! Herdr injects environment variables into every process it starts in a pane.
//! Those are the whole discovery mechanism: no config file, no handshake, no
//! search. If they are absent, this process is not in a pane and there is
//! nothing to report to.
//!
//! Verified against `herdr 0.8.0` via `herdr api schema --json`, which is worth
//! re-running rather than trusting this comment — it is a snapshot of one
//! version of someone else's protocol.

use std::path::PathBuf;

/// The pane this process is running in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    /// Herdr's id for this pane, e.g. `w1:p1`.
    pub id: String,
    /// The session socket to send requests to.
    pub socket: PathBuf,
}

/// Herdr's id for the pane a process was started in.
const PANE_ID_ENV: &str = "HERDR_PANE_ID";
/// The session socket, when it is not the default.
const SOCKET_ENV: &str = "HERDR_SOCKET_PATH";

/// How kamiroh identifies itself when reporting.
///
/// Herdr reserves the `custom:` prefix for sources it does not itself
/// implement, which is exactly what this is.
pub const REPORT_SOURCE: &str = "custom:kamiroh";

impl Pane {
    /// Reads the pane from the environment, or `None` if not inside Herdr.
    ///
    /// `HERDR_PANE_ID` is the signal. `HERDR_ENV=1` is also injected and would
    /// serve, but the pane id is the thing actually *needed* — treating the
    /// value we cannot proceed without as the test avoids a state where the
    /// marker says yes and the id is missing.
    pub fn from_env() -> Option<Self> {
        let id = non_empty(PANE_ID_ENV)?;
        Some(Self {
            id,
            socket: socket_path()?,
        })
    }
}

/// `$HERDR_SOCKET_PATH`, else `~/.config/herdr/herdr.sock`.
///
/// Public because driving a Herdr *agent* needs the socket but not a pane of
/// one's own: kamiroh can sit outside Herdr and still prompt an agent inside it.
pub fn socket_path() -> Option<PathBuf> {
    if let Some(path) = non_empty(SOCKET_ENV) {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("herdr")
            .join("herdr.sock"),
    )
}

/// Reads an environment variable, treating empty as unset.
fn non_empty(key: &str) -> Option<String> {
    let value = std::env::var_os(key)?;
    let value = value.to_string_lossy().into_owned();
    (!value.trim().is_empty()).then_some(value)
}

/// A pane's agent state, in Herdr's vocabulary.
///
/// The mapping from kamiroh's [`AgentStatus`](kamiroh_domain::AgentStatus) is
/// not quite onto, in both directions, and both gaps are decisions rather than
/// oversights:
///
/// - **`Blocked` maps straight through**, as of M1. It was unreachable until
///   `AgentStatus::Blocked` existed; the note here used to say "when one does,
///   this is where it surfaces". This is where it surfaced.
/// - **`Starting` maps to `Unknown`, not `Idle`.** An agent that is not yet
///   ready is not idle, and a sidebar reading "idle" would invite someone to
///   prompt it. `Unknown` is the only value that asserts nothing false. In
///   practice this is dead today: `KameoController` spawns actors already idle,
///   so `Starting` is unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneAgentState {
    /// Ready, not working.
    Idle,
    /// Working on something.
    Working,
    /// Waiting on a human.
    Blocked,
    /// Finished, or stopped.
    Done,
    /// Not determinable.
    Unknown,
}

impl PaneAgentState {
    /// Reads a state Herdr reported back.
    ///
    /// Anything unrecognised becomes [`Unknown`](Self::Unknown) rather than an
    /// error: this is Herdr's vocabulary, Herdr may add to it, and a state we
    /// do not know is precisely what "unknown" is for.
    pub fn from_wire(text: &str) -> Self {
        match text {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "blocked" => Self::Blocked,
            "done" => Self::Done,
            _ => Self::Unknown,
        }
    }

    /// The wire form Herdr expects.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }
}

impl From<kamiroh_domain::AgentStatus> for PaneAgentState {
    fn from(status: kamiroh_domain::AgentStatus) -> Self {
        use kamiroh_domain::AgentStatus;
        match status {
            AgentStatus::Starting => Self::Unknown,
            AgentStatus::Idle => Self::Idle,
            AgentStatus::Busy => Self::Working,
            AgentStatus::Blocked => Self::Blocked,
            AgentStatus::Stopped => Self::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use kamiroh_domain::AgentStatus;

    use super::*;

    #[test]
    fn every_state_has_the_wire_spelling_herdr_documents() {
        // The five values from `herdr api schema --json`, lowercase and bare.
        assert_eq!(PaneAgentState::Idle.as_str(), "idle");
        assert_eq!(PaneAgentState::Working.as_str(), "working");
        assert_eq!(PaneAgentState::Blocked.as_str(), "blocked");
        assert_eq!(PaneAgentState::Done.as_str(), "done");
        assert_eq!(PaneAgentState::Unknown.as_str(), "unknown");
    }

    #[test]
    fn agent_status_maps_onto_herdr_states() {
        assert_eq!(
            PaneAgentState::from(AgentStatus::Idle),
            PaneAgentState::Idle
        );
        assert_eq!(
            PaneAgentState::from(AgentStatus::Busy),
            PaneAgentState::Working
        );
        assert_eq!(
            PaneAgentState::from(AgentStatus::Stopped),
            PaneAgentState::Done
        );
    }

    #[test]
    fn a_starting_agent_is_unknown_rather_than_idle() {
        // Reporting `idle` would invite someone to prompt an agent that is not
        // ready. This is the one mapping worth a test of its own.
        assert_eq!(
            PaneAgentState::from(AgentStatus::Starting),
            PaneAgentState::Unknown
        );
    }
}
