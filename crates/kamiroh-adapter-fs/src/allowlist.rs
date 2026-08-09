//! Filesystem-backed [`Allowlist`].
//!
//! # Storage
//!
//! One endpoint id per line, as 64 lowercase hex characters — the same form
//! the node prints for itself, so admitting a peer is copy-and-paste. `#`
//! starts a comment, to the end of the line; blank lines are ignored.
//!
//! ```text
//! # laptop
//! cb1b755a7d4d6330665717449a886d58270b289746135c33d531038846dc9141
//! c599f4f283de4546...  # workstation
//! ```
//!
//! # Failure is refusal to start, not a guess
//!
//! A file that exists but cannot be read or parsed is an error, and the binary
//! stops. This matches the key store's rule that a corrupt file is reported
//! rather than worked around, and for the same reason: the allowlist *is* the
//! trust boundary, so a malformed one means the operator's intent is unknown.
//! Starting anyway would mean choosing a security policy on their behalf —
//! either admitting a partial list, or admitting nobody while looking healthy.
//!
//! A file that is simply **absent** is not an error. It means the same thing as
//! an empty one: admit nobody. That is the deny-by-default the port requires,
//! and it is what a fresh node has before anyone configures it.
//!
//! # Permissions: integrity, not secrecy
//!
//! An allowlist holds public keys, so unlike the node secret it does not need
//! hiding — anyone may read it. It must not be *writable* by anyone else,
//! though: an account that can append a line can admit itself to this node.
//! Both the file and its parent directory are checked for group and other
//! write access, and neither is required to be unreadable.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use kamiroh_domain::{EndpointId, ParseEndpointIdError};
use kamiroh_ports::Allowlist;

/// An allowlist read from a file.
///
/// The set is held behind an `RwLock` so [`reload`](Self::reload) can replace it
/// atomically while [`is_allowed`](Allowlist::is_allowed) calls are in flight.
/// Checks take the read lock, so concurrent ones do not contend.
#[derive(Debug)]
pub struct FileAllowlist {
    path: PathBuf,
    allowed: RwLock<HashSet<EndpointId>>,
}

impl FileAllowlist {
    /// Reads `path` and holds the result.
    ///
    /// An absent file yields an allowlist permitting nobody. Anything else that
    /// goes wrong is an error — see the module docs.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, AllowlistError> {
        let path = path.into();
        let allowed = read(&path)?;
        // The count, never the entries: an allowlist is public keys, but a log
        // is a different audience from a config file, and "how many" is what a
        // startup check actually needs.
        tracing::info!(path = %path.display(), peers = allowed.len(), "allowlist loaded");
        Ok(Self {
            path,
            allowed: RwLock::new(allowed),
        })
    }

    /// Re-reads the file and replaces the set, returning how many are permitted.
    ///
    /// **A failed reload changes nothing.** The file is read and parsed before
    /// the write lock is taken, so an unreadable or malformed file leaves the
    /// previously loaded set in place and returns the error. The caller decides
    /// what that means — this type will neither empty the list nor keep quiet
    /// about the failure, because those are opposite risks and only the caller
    /// knows which one it is running: retaining a stale list can miss a
    /// revocation, while emptying one locks out every peer over a typo.
    ///
    /// Nothing triggers this yet; a node loads its allowlist at startup. It
    /// exists because the atomic swap is the part that is hard to add later,
    /// whereas a trigger — a signal, a file watch, a Herdr command — is not.
    pub fn reload(&self) -> Result<usize, AllowlistError> {
        let next = read(&self.path)?;
        let mut current = self.allowed.write().expect("allowlist lock poisoned");
        *current = next;
        Ok(current.len())
    }

    /// The conventional location: `$XDG_CONFIG_HOME/kamiroh/allow`, or
    /// `$HOME/.config/kamiroh/allow`.
    ///
    /// Beside `node.key`, since both are this node's identity: one is who it
    /// is, the other is who it will talk to.
    pub fn default_path() -> Result<PathBuf, AllowlistError> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                let home = std::env::var_os("HOME").ok_or(AllowlistError::Unconfigured {
                    reason: "neither XDG_CONFIG_HOME nor HOME is set".to_owned(),
                })?;
                PathBuf::from(home).join(".config")
            }
        };
        Ok(base.join("kamiroh").join("allow"))
    }

    /// The file this allowlist was read from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many endpoints are permitted.
    pub fn len(&self) -> usize {
        self.allowed.read().expect("allowlist lock poisoned").len()
    }

    /// Whether the list permits nobody.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Allowlist for FileAllowlist {
    fn is_allowed(&self, endpoint: &EndpointId) -> bool {
        self.allowed
            .read()
            .expect("allowlist lock poisoned")
            .contains(endpoint)
    }
}

/// Why an allowlist file could not be turned into a set of endpoints.
///
/// Adapter-local on purpose: the [`Allowlist`] port is infallible, because a
/// membership check that can fail invites a caller to treat the failure as
/// "allow". Loading is a separate act that happens once, before any check.
#[derive(Debug, thiserror::Error)]
pub enum AllowlistError {
    /// The file exists but could not be read.
    #[error("allowlist {path} could not be read: {source}")]
    Unreadable {
        /// The file that could not be read.
        path: PathBuf,
        /// What the filesystem reported.
        #[source]
        source: std::io::Error,
    },

    /// A line was not an endpoint id.
    ///
    /// The offending text is included: an allowlist is public keys, so quoting
    /// it leaks nothing and is the difference between a usable error and a
    /// scavenger hunt.
    #[error("allowlist {path} line {line}: {entry:?} is not an endpoint id: {source}")]
    Malformed {
        /// The file being read.
        path: PathBuf,
        /// The 1-based line number.
        line: usize,
        /// The text that failed to parse.
        entry: String,
        /// Why it failed.
        #[source]
        source: ParseEndpointIdError,
    },

    /// The file or its directory was writable by more than its owner.
    #[error("allowlist permissions are too permissive: {detail}")]
    InsecurePermissions {
        /// What was wrong with the permissions.
        detail: String,
    },

    /// No default location could be derived from the environment.
    #[error("cannot locate the default allowlist: {reason}")]
    Unconfigured {
        /// What was missing.
        reason: String,
    },
}

/// Reads and parses `path`, treating an absent file as "permit nobody".
fn read(path: &Path) -> Result<HashSet<EndpointId>, AllowlistError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        // Absent is a configuration state, not a failure: deny everyone.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(source) => {
            return Err(AllowlistError::Unreadable {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    // Before reading, not after: a file anyone can rewrite is not evidence of
    // anything, so there is no point parsing it first.
    imp::check_file_permissions(path, &metadata)?;
    if let Some(parent) = path.parent() {
        imp::check_dir_permissions(parent)?;
    }

    let contents = std::fs::read_to_string(path).map_err(|source| AllowlistError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    parse(path, &contents)
}

/// Turns file contents into a set, rejecting the whole file on a bad line.
fn parse(path: &Path, contents: &str) -> Result<HashSet<EndpointId>, AllowlistError> {
    let mut allowed = HashSet::new();

    for (index, raw) in contents.lines().enumerate() {
        let entry = raw.split_once('#').map_or(raw, |(before, _)| before).trim();
        if entry.is_empty() {
            continue;
        }

        let endpoint = entry
            .parse::<EndpointId>()
            .map_err(|source| AllowlistError::Malformed {
                path: path.to_path_buf(),
                line: index + 1,
                entry: entry.to_owned(),
                source,
            })?;

        // Duplicates are silently fine — a set is a set, and the same peer
        // listed twice expresses no contradiction worth rejecting a file over.
        allowed.insert(endpoint);
    }

    Ok(allowed)
}

#[cfg(unix)]
mod imp {
    use super::{AllowlistError, Path};
    use std::os::unix::fs::PermissionsExt;

    /// Rejects an allowlist any other account can write to.
    ///
    /// Deliberately not the key store's check. That one also rejects *readable*
    /// files, which is right for a secret and wrong here: an allowlist is public
    /// keys, and demanding `0600` would be security theatre that makes the file
    /// harder to inspect for no gain.
    pub(super) fn check_file_permissions(
        path: &Path,
        metadata: &std::fs::Metadata,
    ) -> Result<(), AllowlistError> {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o022 != 0 {
            return Err(AllowlistError::InsecurePermissions {
                detail: format!(
                    "{} is mode {mode:04o}; a group- or other-writable allowlist lets another \
                     account admit itself to this node",
                    path.display()
                ),
            });
        }
        Ok(())
    }

    /// Rejects a directory others may write to.
    ///
    /// Write access to the directory is as good as write access to the file:
    /// it allows replacing the allowlist wholesale.
    pub(super) fn check_dir_permissions(path: &Path) -> Result<(), AllowlistError> {
        if path.as_os_str().is_empty() {
            return Ok(());
        }
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(AllowlistError::Unreadable {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o022 != 0 {
            return Err(AllowlistError::InsecurePermissions {
                detail: format!(
                    "{} is mode {mode:04o}; a group- or other-writable directory lets another \
                     account replace the allowlist",
                    path.display()
                ),
            });
        }
        Ok(())
    }
}

#[cfg(not(unix))]
mod imp {
    use super::{AllowlistError, Path};

    // The integrity rule is expressed in Unix mode bits. Elsewhere the
    // allowlist still works, but callers are responsible for the equivalent
    // ACLs — the same position the key store takes.

    pub(super) fn check_file_permissions(
        _path: &Path,
        _metadata: &std::fs::Metadata,
    ) -> Result<(), AllowlistError> {
        Ok(())
    }

    pub(super) fn check_dir_permissions(_path: &Path) -> Result<(), AllowlistError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic id, and its hex form as it would appear in the file.
    fn endpoint(byte: u8) -> EndpointId {
        EndpointId::from_bytes([byte; 32])
    }

    fn hex(byte: u8) -> String {
        endpoint(byte).to_string()
    }

    /// Writes `contents` to a fresh directory and loads it.
    fn load(contents: &str) -> (tempfile::TempDir, Result<FileAllowlist, AllowlistError>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allow");
        std::fs::write(&path, contents).unwrap();
        let loaded = FileAllowlist::load(&path);
        (dir, loaded)
    }

    #[test]
    fn an_absent_file_permits_nobody() {
        let dir = tempfile::tempdir().unwrap();
        let allowlist = FileAllowlist::load(dir.path().join("does-not-exist")).unwrap();

        assert!(allowlist.is_empty());
        for byte in [0, 1, 42, 255] {
            assert!(!allowlist.is_allowed(&endpoint(byte)));
        }
    }

    #[test]
    fn listed_endpoints_are_permitted_and_others_are_not() {
        let (_dir, allowlist) = load(&format!("{}\n{}\n", hex(1), hex(2)));
        let allowlist = allowlist.unwrap();

        assert_eq!(allowlist.len(), 2);
        assert!(allowlist.is_allowed(&endpoint(1)));
        assert!(allowlist.is_allowed(&endpoint(2)));
        assert!(!allowlist.is_allowed(&endpoint(3)));
    }

    #[test]
    fn comments_blank_lines_and_whitespace_are_ignored() {
        let contents = format!(
            "# peers we trust\n\
             \n   \n\
             {}   # laptop\n\
             \t{}\t\n\
             # {}\n",
            hex(1),
            hex(2),
            hex(9)
        );
        let (_dir, allowlist) = load(&contents);
        let allowlist = allowlist.unwrap();

        assert_eq!(allowlist.len(), 2);
        assert!(allowlist.is_allowed(&endpoint(1)));
        assert!(allowlist.is_allowed(&endpoint(2)));
        assert!(
            !allowlist.is_allowed(&endpoint(9)),
            "a commented-out entry must not be admitted"
        );
    }

    #[test]
    fn an_endpoint_listed_twice_is_not_an_error() {
        let (_dir, allowlist) = load(&format!("{}\n{}\n", hex(1), hex(1)));
        let allowlist = allowlist.unwrap();

        assert_eq!(allowlist.len(), 1);
        assert!(allowlist.is_allowed(&endpoint(1)));
    }

    #[test]
    fn a_malformed_line_rejects_the_whole_file() {
        let (_dir, loaded) = load(&format!("{}\nnot-an-endpoint-id\n{}\n", hex(1), hex(2)));

        let error = loaded.unwrap_err();
        let AllowlistError::Malformed { line, entry, .. } = &error else {
            panic!("expected Malformed, got {error:?}");
        };
        assert_eq!(*line, 2, "the reported line must be 1-based");
        assert_eq!(entry, "not-an-endpoint-id");
    }

    /// The whole point of refusing to start: a file we cannot fully understand
    /// must not silently become a shorter allowlist.
    #[test]
    fn a_malformed_file_yields_no_partial_allowlist() {
        let (_dir, loaded) = load(&format!("{}\ntruncated-id\n", hex(1)));
        assert!(
            loaded.is_err(),
            "a file whose first line parses must still be rejected as a whole"
        );
    }

    #[test]
    fn reload_picks_up_an_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allow");
        std::fs::write(&path, format!("{}\n", hex(1))).unwrap();

        let allowlist = FileAllowlist::load(&path).unwrap();
        assert!(allowlist.is_allowed(&endpoint(1)));
        assert!(!allowlist.is_allowed(&endpoint(2)));

        std::fs::write(&path, format!("{}\n", hex(2))).unwrap();
        assert_eq!(allowlist.reload().unwrap(), 1);

        assert!(!allowlist.is_allowed(&endpoint(1)), "revocation must apply");
        assert!(allowlist.is_allowed(&endpoint(2)));
    }

    /// The documented contract: a bad reload is inert, not destructive.
    #[test]
    fn a_failed_reload_leaves_the_previous_set_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allow");
        std::fs::write(&path, format!("{}\n", hex(1))).unwrap();
        let allowlist = FileAllowlist::load(&path).unwrap();

        std::fs::write(&path, "garbage\n").unwrap();
        assert!(allowlist.reload().is_err());

        assert!(
            allowlist.is_allowed(&endpoint(1)),
            "a failed reload must not empty the allowlist"
        );
        assert_eq!(allowlist.len(), 1);
    }

    #[test]
    fn reload_after_the_file_is_deleted_denies_everyone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allow");
        std::fs::write(&path, format!("{}\n", hex(1))).unwrap();
        let allowlist = FileAllowlist::load(&path).unwrap();

        // Deleting is a legible instruction — "admit nobody" — unlike a parse
        // failure, so it applies rather than being retained.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(allowlist.reload().unwrap(), 0);
        assert!(!allowlist.is_allowed(&endpoint(1)));
    }

    /// The property `SIGHUP` reload depends on: a fumbled edit costs a log line
    /// rather than every peer. Already covered above for the error path; this
    /// pins that the *path* is retained too, so a signal handler can keep using
    /// the same handle.
    #[test]
    fn a_reloadable_allowlist_remembers_where_it_came_from() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allow");
        std::fs::write(&path, format!("{}\n", hex(1))).unwrap();

        let allowlist = FileAllowlist::load(&path).unwrap();
        assert_eq!(allowlist.path(), path);

        std::fs::write(&path, format!("{}\n{}\n", hex(1), hex(2))).unwrap();
        assert_eq!(allowlist.reload().unwrap(), 2);
        assert_eq!(allowlist.path(), path, "the path must survive a reload");
    }

    #[test]
    fn an_empty_file_permits_nobody() {
        let (_dir, allowlist) = load("");
        assert!(allowlist.unwrap().is_empty());
    }

    #[test]
    fn a_file_of_only_comments_permits_nobody() {
        let (_dir, allowlist) = load("# nobody yet\n\n# still nobody\n");
        assert!(allowlist.unwrap().is_empty());
    }

    #[cfg(unix)]
    mod unix {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        fn set_mode(path: &Path, mode: u32) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        #[test]
        fn a_writable_allowlist_is_refused() {
            for mode in [0o666, 0o646, 0o662, 0o622] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("allow");
                std::fs::write(&path, format!("{}\n", hex(1))).unwrap();
                set_mode(&path, mode);

                let error = FileAllowlist::load(&path).unwrap_err();
                assert!(
                    matches!(error, AllowlistError::InsecurePermissions { .. }),
                    "mode {mode:04o} must be refused, got {error:?}"
                );
            }
        }

        #[test]
        fn a_readable_allowlist_is_fine() {
            // The opposite of the key store's rule, and deliberately so: these
            // are public keys, and a world-readable allowlist leaks nothing.
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("allow");
            std::fs::write(&path, format!("{}\n", hex(1))).unwrap();
            set_mode(&path, 0o644);

            let allowlist = FileAllowlist::load(&path).unwrap();
            assert!(allowlist.is_allowed(&endpoint(1)));
        }

        #[test]
        fn a_writable_directory_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            let nested = dir.path().join("config");
            std::fs::create_dir(&nested).unwrap();
            let path = nested.join("allow");
            std::fs::write(&path, format!("{}\n", hex(1))).unwrap();
            set_mode(&path, 0o600);
            set_mode(&nested, 0o777);

            let error = FileAllowlist::load(&path).unwrap_err();
            assert!(
                matches!(error, AllowlistError::InsecurePermissions { .. }),
                "a world-writable directory must be refused, got {error:?}"
            );

            // Restore, or the TempDir cannot be cleaned up.
            set_mode(&nested, 0o700);
        }

        /// Permissions are checked before parsing: a file anyone can rewrite is
        /// not evidence, so its contents are beside the point.
        #[test]
        fn permissions_are_checked_before_contents() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("allow");
            std::fs::write(&path, "definitely not an endpoint id\n").unwrap();
            set_mode(&path, 0o666);

            let error = FileAllowlist::load(&path).unwrap_err();
            assert!(
                matches!(error, AllowlistError::InsecurePermissions { .. }),
                "expected the permission failure to win, got {error:?}"
            );
        }
    }
}
