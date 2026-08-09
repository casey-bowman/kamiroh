//! Filesystem-backed [`KeyStore`].
//!
//! # Storage
//!
//! One file holding the node secret as 64 lowercase hex characters and a
//! trailing newline. Hex rather than raw bytes so the file is inspectable, is
//! never mistaken for corrupt binary, and reads the same way as an
//! [`EndpointId`](kamiroh_domain::EndpointId).
//!
//! # Creation publishes with `hard_link`, never `rename` and never in place
//!
//! A new key is written to a temporary file beside the target, flushed, and then
//! published with [`std::fs::hard_link`]. Publishing a node identity needs two
//! properties at once, and the two obvious approaches each supply only one:
//!
//! | | non-clobbering | atomic publish |
//! |---|---|---|
//! | write temp, `rename` into place | ✗ destroys an existing identity | ✓ |
//! | `O_CREAT \| O_EXCL` on the final path, then write | ✓ | ✗ name exists before contents do |
//! | write temp, `hard_link` into place | ✓ fails `EEXIST` | ✓ links after fsync |
//!
//! The middle row is the trap: it looks safe, but a second process starting at
//! the same moment opens the newly created name and reads **zero bytes**. That
//! is a genuine startup race, and `concurrent_creators_converge_on_one_secret`
//! reproduces it.
//!
//! Losing the race is not an error: the loser deletes its own candidate and
//! reads the winner's key, which is complete because the winner linked only
//! after its `fsync`.
//!
//! # Blocking I/O
//!
//! The port is async but these calls block. A node loads its key once at
//! startup, so a dedicated thread pool would buy nothing.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kamiroh_domain::NodeSecret;
use kamiroh_domain::secret::NODE_SECRET_LEN;
use kamiroh_ports::{KeyStore, KeyStoreError};

use crate::wipe::{WipedArray, WipedVec};

/// Bytes on disk: the hex form plus one newline.
const FILE_LEN: usize = NODE_SECRET_LEN * 2 + 1;

/// A node secret stored in a file.
#[derive(Debug, Clone)]
pub struct FileKeyStore {
    path: PathBuf,
}

impl FileKeyStore {
    /// Creates a store backed by `path`.
    ///
    /// The file and its parent directory are created on first use if missing.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file this store reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The conventional location: `$XDG_CONFIG_HOME/kamiroh/node.key`, or
    /// `$HOME/.config/kamiroh/node.key`.
    pub fn default_path() -> Result<PathBuf, KeyStoreError> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                let home = std::env::var_os("HOME").ok_or(KeyStoreError::Malformed {
                    reason: "neither XDG_CONFIG_HOME nor HOME is set".to_owned(),
                })?;
                PathBuf::from(home).join(".config")
            }
        };
        Ok(base.join("kamiroh").join("node.key"))
    }

    /// Reads the stored secret, or `Ok(None)` if the file does not exist.
    fn load(&self) -> Result<Option<NodeSecret>, KeyStoreError> {
        let metadata = match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(backend(error)),
        };

        // Permissions are checked before the file is opened. Reading first and
        // erroring afterwards would pull the secret into this process even
        // though we had already decided it was compromised.
        check_file_permissions(&self.path, &metadata)?;
        if let Some(parent) = self.path.parent() {
            check_dir_permissions(parent)?;
        }

        let contents = WipedVec::new(std::fs::read(&self.path).map_err(backend)?);
        let trimmed = trim_ascii_end(&contents);

        NodeSecret::from_hex(trimmed)
            .map(Some)
            .map_err(|error| KeyStoreError::Malformed {
                // `ParseNodeSecretError` is built to describe key material
                // without quoting it, so this is safe to surface and log.
                reason: format!("{} contains a malformed key: {error}", self.path.display()),
            })
    }

    /// Generates and stores a new secret, or `Ok(None)` if another writer won.
    ///
    /// The secret is written to a temporary file in the same directory, flushed,
    /// and only then published with [`std::fs::hard_link`]. That gives both
    /// properties this path needs at once:
    ///
    /// - **Non-clobbering.** `link` fails with `AlreadyExists` rather than
    ///   overwriting, so a concurrent starter can never destroy an established
    ///   identity the way `rename` would.
    /// - **Atomic publish.** The final name appears only once the bytes are on
    ///   disk. Creating the final path with `O_CREAT | O_EXCL` and writing
    ///   afterwards looks safe but is not: it publishes the name first, and a
    ///   concurrent reader observes a zero-length file. That is a real startup
    ///   race — two nodes launching together — and it is what the concurrency
    ///   test reproduces.
    fn create(&self) -> Result<Option<NodeSecret>, KeyStoreError> {
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent)?;
        }

        let secret = NodeSecret::from_fill(|bytes| getrandom::fill(bytes))
            .map_err(|error| KeyStoreError::Backend(Box::new(error)))?;

        let mut contents = WipedArray::<FILE_LEN>::zeroed();
        let (hex, newline) = contents.split_at_mut(NODE_SECRET_LEN * 2);
        secret.write_hex_into(hex.try_into().expect("hex slice is exactly 2n bytes"));
        newline[0] = b'\n';

        // Removed on every exit path, including the error ones: a temp file left
        // behind would be a live secret loose in the key directory.
        let temp = ScopedTempFile::create(self.temp_path())?;

        {
            let mut file = temp.file();
            file.write_all(&*contents).map_err(backend)?;
            file.sync_all().map_err(backend)?;
        }

        match std::fs::hard_link(temp.path(), &self.path) {
            Ok(()) => {}
            // Lost the race. The winner linked only after its fsync, so the
            // caller's re-load is guaranteed to see a complete file.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
            Err(error) => return Err(backend(error)),
        }

        if let Some(parent) = self.path.parent() {
            sync_dir(parent)?;
        }

        Ok(Some(secret))
    }

    /// A staging path beside the key file, unique to this attempt.
    ///
    /// The pid separates processes and the counter separates threads within one
    /// — a pid alone is not enough, since several tasks in the same process can
    /// race here and would otherwise stage onto a single path and delete each
    /// other's candidate. No RNG is needed: `O_EXCL` catches any collision that
    /// survives both.
    fn temp_path(&self) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ATTEMPT: AtomicU64 = AtomicU64::new(0);

        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(
            ".tmp.{}.{}",
            std::process::id(),
            ATTEMPT.fetch_add(1, Ordering::Relaxed)
        ));
        match self.path.parent() {
            Some(parent) => parent.join(name),
            None => PathBuf::from(name),
        }
    }
}

/// A private temporary file that deletes itself on drop.
struct ScopedTempFile {
    path: PathBuf,
    file: File,
}

impl ScopedTempFile {
    /// Creates `path` exclusively at mode `0600`.
    ///
    /// A pre-existing file at this path can only be debris from a crashed run
    /// with the same pid: paths are unique per live attempt. It was never linked
    /// into place, so nothing depends on it, and it is replaced.
    fn create(path: PathBuf) -> Result<Self, KeyStoreError> {
        let file = match open_new_private(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                std::fs::remove_file(&path).map_err(backend)?;
                open_new_private(&path).map_err(backend)?
            }
            Err(error) => return Err(backend(error)),
        };
        Ok(Self { path, file })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn file(&self) -> &File {
        &self.file
    }
}

impl Drop for ScopedTempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[async_trait]
impl KeyStore for FileKeyStore {
    async fn load_or_create(&self) -> Result<NodeSecret, KeyStoreError> {
        if let Some(secret) = self.load()? {
            return Ok(secret);
        }
        if let Some(secret) = self.create()? {
            return Ok(secret);
        }
        // Another writer created the file between our load and our create.
        self.load()?.ok_or(KeyStoreError::Missing)
    }
}

fn backend(error: std::io::Error) -> KeyStoreError {
    KeyStoreError::Backend(Box::new(error))
}

/// Drops trailing ASCII whitespace, so an editor's stray newline is tolerated
/// while the hex itself is still length-checked strictly.
fn trim_ascii_end(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &bytes[..end]
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    /// Rejects a key file any account but its owner can read or write.
    pub(super) fn check_file_permissions(
        path: &Path,
        metadata: &std::fs::Metadata,
    ) -> Result<(), KeyStoreError> {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(KeyStoreError::InsecurePermissions {
                detail: format!(
                    "{} is mode {mode:04o}; a key file must grant no group or other access (0600)",
                    path.display()
                ),
            });
        }
        Ok(())
    }

    /// Rejects a directory others may write to.
    ///
    /// Read access to the directory is harmless — the key file itself is `0600`.
    /// Write access is not: it lets another account replace the key file wholesale,
    /// substituting an identity even though it could never read the original.
    pub(super) fn check_dir_permissions(path: &Path) -> Result<(), KeyStoreError> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(backend(error)),
        };
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o022 != 0 {
            return Err(KeyStoreError::InsecurePermissions {
                detail: format!(
                    "{} is mode {mode:04o}; a group- or other-writable directory lets another \
                     account replace the key file",
                    path.display()
                ),
            });
        }
        Ok(())
    }

    /// Creates the directory at `0700` if absent, else checks what is there.
    pub(super) fn ensure_private_dir(path: &Path) -> Result<(), KeyStoreError> {
        if path.as_os_str().is_empty() {
            return Ok(());
        }
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(backend)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .map_err(backend)?;
            return Ok(());
        }
        check_dir_permissions(path)
    }

    /// Opens `path` for writing, failing if it exists, at mode `0600`.
    ///
    /// The mode is set at creation rather than afterwards: a `chmod` after the
    /// fact leaves a window in which the key is world-readable.
    pub(super) fn open_new_private(path: &Path) -> std::io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }

    /// Flushes a directory entry, so a created key survives a crash.
    pub(super) fn sync_dir(path: &Path) -> Result<(), KeyStoreError> {
        if path.as_os_str().is_empty() {
            return Ok(());
        }
        File::open(path)
            .and_then(|dir| dir.sync_all())
            .map_err(backend)
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;

    // kamiroh's key custody rules are expressed in Unix mode bits. On other
    // platforms the store still works, but it cannot enforce them, so callers
    // are responsible for the equivalent ACLs.

    pub(super) fn check_file_permissions(
        _path: &Path,
        _metadata: &std::fs::Metadata,
    ) -> Result<(), KeyStoreError> {
        Ok(())
    }

    pub(super) fn check_dir_permissions(_path: &Path) -> Result<(), KeyStoreError> {
        Ok(())
    }

    pub(super) fn ensure_private_dir(path: &Path) -> Result<(), KeyStoreError> {
        if path.as_os_str().is_empty() || path.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(path).map_err(backend)
    }

    pub(super) fn open_new_private(path: &Path) -> std::io::Result<File> {
        OpenOptions::new().write(true).create_new(true).open(path)
    }

    pub(super) fn sync_dir(_path: &Path) -> Result<(), KeyStoreError> {
        Ok(())
    }
}

use imp::{
    check_dir_permissions, check_file_permissions, ensure_private_dir, open_new_private, sync_dir,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> FileKeyStore {
        FileKeyStore::new(dir.path().join("node.key"))
    }

    #[tokio::test]
    async fn creates_a_secret_when_none_exists() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);

        let secret = store.load_or_create().await.unwrap();

        assert!(store.path().exists());
        // Real entropy, not a fixed development value.
        assert_ne!(secret.expose_bytes(), &[0u8; NODE_SECRET_LEN]);
    }

    #[tokio::test]
    async fn identity_is_stable_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);

        let first = store.load_or_create().await.unwrap();
        let second = store.load_or_create().await.unwrap();

        assert_eq!(first.expose_bytes(), second.expose_bytes());

        // And across a fresh store on the same path — a node restart.
        let restarted = FileKeyStore::new(store.path())
            .load_or_create()
            .await
            .unwrap();
        assert_eq!(first.expose_bytes(), restarted.expose_bytes());
    }

    #[tokio::test]
    async fn separate_paths_get_separate_identities() {
        let dir = tempfile::tempdir().unwrap();
        let one = FileKeyStore::new(dir.path().join("one.key"))
            .load_or_create()
            .await
            .unwrap();
        let two = FileKeyStore::new(dir.path().join("two.key"))
            .load_or_create()
            .await
            .unwrap();
        assert_ne!(one.expose_bytes(), two.expose_bytes());
    }

    #[tokio::test]
    async fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileKeyStore::new(dir.path().join("nested").join("deeper").join("node.key"));

        store.load_or_create().await.unwrap();

        assert!(store.path().exists());
    }

    #[tokio::test]
    async fn stored_file_is_hex_with_a_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let secret = store.load_or_create().await.unwrap();

        let raw = std::fs::read(store.path()).unwrap();
        assert_eq!(raw.len(), FILE_LEN);
        assert_eq!(raw[FILE_LEN - 1], b'\n');

        let mut expected = [0u8; NODE_SECRET_LEN * 2];
        secret.write_hex_into(&mut expected);
        assert_eq!(&raw[..NODE_SECRET_LEN * 2], &expected);
    }

    #[tokio::test]
    async fn a_trailing_newline_is_optional_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let secret = store.load_or_create().await.unwrap();

        let raw = std::fs::read(store.path()).unwrap();
        std::fs::write(store.path(), &raw[..NODE_SECRET_LEN * 2]).unwrap();
        restore_private_mode(store.path());

        let reloaded = store.load_or_create().await.unwrap();
        assert_eq!(reloaded.expose_bytes(), secret.expose_bytes());
    }

    #[tokio::test]
    async fn malformed_contents_are_rejected_without_leaking_them() {
        for (label, contents) in [
            ("too short", "abcd".to_owned()),
            ("not hex", "zz".repeat(NODE_SECRET_LEN)),
            ("empty", String::new()),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let store = store(&dir);
            std::fs::write(store.path(), &contents).unwrap();
            restore_private_mode(store.path());

            let error = store.load_or_create().await.unwrap_err();
            assert!(
                matches!(error, KeyStoreError::Malformed { .. }),
                "{label}: {error:?}"
            );

            // A malformed file may still hold most of a real key; the error must
            // not quote any of it.
            let rendered = error.to_string();
            assert!(!rendered.contains("zz"), "{label}: {rendered}");
            assert!(!rendered.contains("abcd"), "{label}: {rendered}");
        }
    }

    #[tokio::test]
    async fn an_existing_key_is_never_overwritten_by_a_malformed_read() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        std::fs::write(store.path(), "not a key").unwrap();
        restore_private_mode(store.path());

        assert!(store.load_or_create().await.is_err());
        // The bad file is reported, not silently replaced: a corrupt key file
        // may be a recoverable identity, and clobbering it destroys the node.
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), "not a key");
    }

    #[tokio::test]
    async fn concurrent_creators_converge_on_one_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.key");

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let store = FileKeyStore::new(path);
                    block_on(store.load_or_create())
                })
            })
            .collect();

        let secrets: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap().unwrap())
            .collect();

        let first = secrets[0].expose_bytes();
        for secret in &secrets {
            assert_eq!(
                secret.expose_bytes(),
                first,
                "a concurrent creator clobbered the node identity"
            );
        }

        // Every loser cleaned up after itself: a stranded temp file would be a
        // live secret sitting in the key directory.
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("node.key")]);
    }

    #[tokio::test]
    async fn a_successful_create_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        store.load_or_create().await.unwrap();

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("node.key")]);
    }

    /// Polls a future exactly once, avoiding one Tokio runtime per test thread.
    ///
    /// `load_or_create` is blocking throughout and has no await point that can
    /// pend, so a single poll must complete it. Asserting that is deliberate: if
    /// a later refactor introduces a real suspension, this fails loudly instead
    /// of spinning on a no-op waker.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};

        let mut context = Context::from_waker(Waker::noop());
        match pin!(future).poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("load_or_create must not yield"),
        }
    }

    /// Restores `0600` after a test writes the file with `std::fs::write`, which
    /// uses the process umask rather than our creation mode.
    fn restore_private_mode(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = path;
    }

    #[cfg(unix)]
    mod unix {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        fn mode_of(path: &Path) -> u32 {
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        #[tokio::test]
        async fn a_new_key_file_is_created_owner_only() {
            let dir = tempfile::tempdir().unwrap();
            let store = store(&dir);
            store.load_or_create().await.unwrap();

            assert_eq!(mode_of(store.path()), 0o600);
        }

        #[tokio::test]
        async fn a_created_parent_directory_is_owner_only() {
            let dir = tempfile::tempdir().unwrap();
            let nested = dir.path().join("nested");
            FileKeyStore::new(nested.join("node.key"))
                .load_or_create()
                .await
                .unwrap();

            assert_eq!(mode_of(&nested), 0o700);
        }

        #[tokio::test]
        async fn a_readable_key_file_is_refused_without_being_read() {
            // The case the check exists for: a key restored from a backup or
            // copied by hand, which the create path never touched.
            let dir = tempfile::tempdir().unwrap();
            let store = store(&dir);
            store.load_or_create().await.unwrap();

            for mode in [0o644, 0o604, 0o660, 0o777] {
                std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(mode))
                    .unwrap();

                let error = store.load_or_create().await.unwrap_err();
                assert!(
                    matches!(error, KeyStoreError::InsecurePermissions { .. }),
                    "mode {mode:04o} should be refused, got {error:?}"
                );
                assert!(error.to_string().contains(&format!("{mode:04o}")));
            }
        }

        #[tokio::test]
        async fn a_world_writable_directory_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            let nested = dir.path().join("nested");
            std::fs::create_dir(&nested).unwrap();
            let store = FileKeyStore::new(nested.join("node.key"));
            store.load_or_create().await.unwrap();

            // Others cannot read the 0600 key, but they can replace the file.
            std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o777)).unwrap();

            let error = store.load_or_create().await.unwrap_err();
            assert!(
                matches!(error, KeyStoreError::InsecurePermissions { .. }),
                "{error:?}"
            );

            std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
}
