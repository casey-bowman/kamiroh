//! Node key custody (driven port).
//!
//! Each node owns one long-lived secret. The port returns domain-typed key
//! material ([`NodeSecret`]), never a transport library's key type, so custody
//! policy (file, keyring, HSM) stays swappable.

use async_trait::async_trait;
use kamiroh_domain::NodeSecret;

/// Loads — or on first run creates — this node's secret.
#[async_trait]
pub trait KeyStore: Send + Sync + 'static {
    /// Returns the node secret, generating and persisting one if none exists.
    ///
    /// Implementations that create a secret must persist it before returning, so
    /// that a node keeps a stable [`kamiroh_domain::EndpointId`] across restarts,
    /// and must store it with owner-only permissions.
    async fn load_or_create(&self) -> Result<NodeSecret, KeyStoreError>;
}

/// Why key material could not be loaded or created.
#[derive(Debug, thiserror::Error)]
pub enum KeyStoreError {
    /// The store holds no secret and this store cannot create one.
    #[error("no node secret available and this key store cannot create one")]
    Missing,

    /// No location could be derived from the environment.
    ///
    /// Distinct from [`Malformed`](Self::Malformed), which is about the
    /// *contents* of a store. Neither `XDG_CONFIG_HOME` nor `HOME` being set is
    /// not a corrupt key — it is a machine that has not said where to put one,
    /// and reporting it as corruption sends whoever hits it looking for a
    /// damaged file that does not exist.
    #[error("cannot locate the key store: {reason}")]
    Unconfigured {
        /// What was missing.
        reason: String,
    },

    /// Stored material was not a well-formed secret.
    #[error("stored key material is malformed: {reason}")]
    Malformed {
        /// What was wrong with the stored material. Must not include key bytes.
        reason: String,
    },

    /// The stored secret was readable by more than its owner.
    #[error("key store permissions are too permissive: {detail}")]
    InsecurePermissions {
        /// What was wrong with the permissions.
        detail: String,
    },

    /// The underlying store (file, keyring, ...) failed.
    #[error("key store backend failed: {0}")]
    Backend(#[source] Box<dyn core::error::Error + Send + Sync>),
}
