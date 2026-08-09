//! Which agents this node hosts, read from a file.
//!
//! # Storage
//!
//! One agent per line: a name, whitespace, and where its work happens. `#`
//! starts a comment; blank lines are ignored. Deliberately the same shape as
//! the allowlist, because an operator editing one should not have to learn a
//! second format.
//!
//! ```text
//! # name     target
//! agent      w1:p2       # a Herdr pane
//! reviewer   codex-main  # a Herdr agent, by name
//! notes      echo        # the stand-in, for a node with no runtime
//! ```
//!
//! # This file says *what*, not *how*
//!
//! A target is an opaque string here. Whether `w1:p2` means a Herdr pane, and
//! what `echo` resolves to, is the composition root's business — it is the one
//! crate allowed to know which adapters exist. Resolving targets here would put
//! Herdr in the filesystem adapter, and a second agent runtime would then have
//! to be added in two places.
//!
//! # Absent is one agent, not none
//!
//! A node with no agents file hosts a single agent called `agent`, which is
//! what every kamiroh node did before this existed. That keeps a fresh node
//! useful and every existing configuration working.

use std::path::{Path, PathBuf};

use kamiroh_domain::{ActorName, InvalidActorName};

/// The target meaning "use the in-memory stand-in".
///
/// Not a Herdr target: it is spelled out so a node can be configured with no
/// agent runtime at all, which is what tests and a fresh checkout want.
pub const ECHO_TARGET: &str = "echo";

/// The name a node hosts when it has no agents file.
pub const DEFAULT_AGENT: &str = "agent";

/// One agent: what to call it, and where its work happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpec {
    /// The name peers and the console address it by.
    pub name: ActorName,
    /// Where the work happens, for the composition root to resolve.
    pub target: String,
}

impl AgentSpec {
    /// Whether this agent asks for the in-memory stand-in.
    pub fn is_echo(&self) -> bool {
        self.target.eq_ignore_ascii_case(ECHO_TARGET)
    }
}

/// Reads the agents file, or yields the default single agent if it is absent.
pub fn load(path: &Path) -> Result<Vec<AgentSpec>, AgentsError> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        // Absent means the default, not an empty node. A node that hosts
        // nothing can be asked for, by writing a file with no entries.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(default_agents()),
        Err(source) => {
            return Err(AgentsError::Unreadable {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    parse(path, &contents)
}

/// The single agent a node hosts when nothing says otherwise.
pub fn default_agents() -> Vec<AgentSpec> {
    vec![AgentSpec {
        // Infallible: `agent` is a valid name, pinned by a test below.
        name: ActorName::new(DEFAULT_AGENT).expect("the default agent name is valid"),
        target: ECHO_TARGET.to_owned(),
    }]
}

/// The conventional location: `$XDG_CONFIG_HOME/kamiroh/agents`.
pub fn default_path() -> Result<PathBuf, AgentsError> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var_os("HOME").ok_or(AgentsError::Unconfigured {
                reason: "neither XDG_CONFIG_HOME nor HOME is set".to_owned(),
            })?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("kamiroh").join("agents"))
}

/// Turns file contents into specs, rejecting the whole file on a bad line.
///
/// Fatal rather than partial, for the same reason as the allowlist: a file that
/// cannot be fully understood means the operator's intent is unknown, and
/// hosting *some* of the agents they asked for is its own kind of wrong.
fn parse(path: &Path, contents: &str) -> Result<Vec<AgentSpec>, AgentsError> {
    let mut agents: Vec<AgentSpec> = Vec::new();

    for (index, raw) in contents.lines().enumerate() {
        let line = index + 1;
        let entry = raw.split_once('#').map_or(raw, |(before, _)| before).trim();
        if entry.is_empty() {
            continue;
        }

        let (name, target) =
            entry
                .split_once(char::is_whitespace)
                .ok_or_else(|| AgentsError::Malformed {
                    path: path.to_path_buf(),
                    line,
                    reason: format!("{entry:?} has no target; expected `<name> <target>`"),
                })?;

        let name = ActorName::new(name).map_err(|source| AgentsError::InvalidName {
            path: path.to_path_buf(),
            line,
            source,
        })?;

        let target = target.trim();
        if target.is_empty() {
            return Err(AgentsError::Malformed {
                path: path.to_path_buf(),
                line,
                reason: format!("{name} has an empty target"),
            });
        }
        // A target is a pane id or an agent name; neither contains whitespace.
        // Without this, `my agent w1:p2` parses as the agent `my` with target
        // `agent w1:p2` — accepted, wrong, and silent. Found by a test whose
        // own input made exactly that mistake.
        if target.split_whitespace().count() > 1 {
            return Err(AgentsError::Malformed {
                path: path.to_path_buf(),
                line,
                reason: format!(
                    "{name} has a target containing spaces ({target:?}); \
                     a name and a target are one word each"
                ),
            });
        }

        // Two agents with one name would make routing ambiguous, and the front
        // routes by name — so this is a real conflict, not a tidiness rule.
        if let Some(first) = agents.iter().find(|spec| spec.name == name) {
            return Err(AgentsError::Duplicate {
                path: path.to_path_buf(),
                line,
                name: first.name.to_string(),
            });
        }

        agents.push(AgentSpec {
            name,
            target: target.to_owned(),
        });
    }

    Ok(agents)
}

/// Why an agents file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum AgentsError {
    /// The file exists but could not be read.
    #[error("agents file {path} could not be read: {source}")]
    Unreadable {
        /// The file.
        path: PathBuf,
        /// What the filesystem reported.
        #[source]
        source: std::io::Error,
    },

    /// A line was not `<name> <target>`.
    #[error("agents file {path} line {line}: {reason}")]
    Malformed {
        /// The file.
        path: PathBuf,
        /// The 1-based line number.
        line: usize,
        /// What was wrong.
        reason: String,
    },

    /// A name was not a valid actor name.
    #[error("agents file {path} line {line}: {source}")]
    InvalidName {
        /// The file.
        path: PathBuf,
        /// The 1-based line number.
        line: usize,
        /// Why the name was rejected.
        #[source]
        source: InvalidActorName,
    },

    /// Two agents share a name, so routing would be ambiguous.
    #[error("agents file {path} line {line}: {name} is listed twice")]
    Duplicate {
        /// The file.
        path: PathBuf,
        /// The 1-based line number of the second entry.
        line: usize,
        /// The repeated name.
        name: String,
    },

    /// No default location could be derived from the environment.
    #[error("cannot locate the agents file: {reason}")]
    Unconfigured {
        /// What was missing.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_from(contents: &str) -> (tempfile::TempDir, Result<Vec<AgentSpec>, AgentsError>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents");
        std::fs::write(&path, contents).unwrap();
        let loaded = load(&path);
        (dir, loaded)
    }

    #[test]
    fn the_default_agent_name_is_valid() {
        // `default_agents` expects this; a rename of `ActorName`'s rules would
        // otherwise turn into a panic at startup.
        assert!(ActorName::new(DEFAULT_AGENT).is_ok());
    }

    #[test]
    fn an_absent_file_means_one_agent_not_none() {
        let dir = tempfile::tempdir().unwrap();
        let agents = load(&dir.path().join("nothing-here")).unwrap();

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name.as_str(), DEFAULT_AGENT);
        assert!(agents[0].is_echo());
    }

    #[test]
    fn an_empty_file_means_a_node_that_hosts_nothing() {
        // Distinct from absent: writing an empty file is a way of asking.
        let (_dir, agents) = load_from("# nobody\n\n");
        assert!(agents.unwrap().is_empty());
    }

    #[test]
    fn several_agents_are_read_in_order() {
        let (_dir, agents) = load_from("agent w1:p2\nreviewer codex-main\nnotes echo\n");
        let agents = agents.unwrap();

        assert_eq!(agents.len(), 3);
        assert_eq!(agents[0].name.as_str(), "agent");
        assert_eq!(agents[0].target, "w1:p2");
        assert_eq!(agents[1].target, "codex-main");
        assert!(agents[2].is_echo());
        assert!(!agents[0].is_echo());
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let (_dir, agents) = load_from("# who\n\n  agent   w1:p2   # the main one\n\t\n");
        let agents = agents.unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].target, "w1:p2");
    }

    /// Routing is by name, so two agents with one name is ambiguity, not
    /// untidiness.
    #[test]
    fn a_repeated_name_is_refused() {
        let (_dir, agents) = load_from("agent w1:p2\nagent w1:p3\n");
        let error = agents.unwrap_err();
        assert!(
            matches!(&error, AgentsError::Duplicate { line: 2, .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_line_without_a_target_says_what_was_expected() {
        let (_dir, agents) = load_from("agent\n");
        let error = agents.unwrap_err();
        let AgentsError::Malformed { reason, line, .. } = &error else {
            panic!("expected Malformed, got {error:?}");
        };
        assert_eq!(*line, 1);
        assert!(reason.contains("<name> <target>"), "{reason}");
    }

    #[test]
    fn an_illegal_name_is_reported_with_its_line() {
        let (_dir, agents) = load_from("agent w1:p2\nbad/name w1:p3\n");
        let error = agents.unwrap_err();
        assert!(
            matches!(&error, AgentsError::InvalidName { line: 2, .. }),
            "got {error:?}"
        );
    }

    /// The trap this exists for: a name with a space in it would otherwise
    /// parse as a *different, valid* agent whose target is the rest of the
    /// line. Accepted, wrong, and silent — the worst combination.
    #[test]
    fn a_spaced_target_is_refused_rather_than_silently_reinterpreted() {
        let (_dir, agents) = load_from("my agent w1:p2\n");
        let error = agents.unwrap_err();
        let AgentsError::Malformed { reason, .. } = &error else {
            panic!("expected Malformed, got {error:?}");
        };
        assert!(reason.contains("spaces"), "{reason}");
    }

    /// One bad line rejects the file, like the allowlist: hosting *some* of the
    /// agents an operator asked for is its own kind of wrong.
    #[test]
    fn a_bad_line_rejects_the_whole_file() {
        let (_dir, agents) = load_from("good w1:p2\nbad\nalso-good w1:p3\n");
        assert!(agents.is_err());
    }
}
