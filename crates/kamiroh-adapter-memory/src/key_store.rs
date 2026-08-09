//! In-memory [`KeyStore`].
//!
//! # Not for production
//!
//! This store keeps a secret in process memory and never persists it, so a node
//! using it changes identity on every restart. It also does not generate key
//! material: callers supply it, or use [`InMemoryKeyStore::insecure_dev`], which
//! returns a **fixed, publicly known** value.
//!
//! Real custody — persistence, owner-only permissions, and a CSPRNG — arrives
//! with the filesystem/keyring adapter in slice E.

use async_trait::async_trait;
use kamiroh_domain::NodeSecret;
use kamiroh_ports::{KeyStore, KeyStoreError};

/// A node secret held only in memory.
///
/// The secret is stored as a [`NodeSecret`], not as bare bytes, so it keeps that
/// type's redacted `Debug` and zero-on-drop behaviour. Storing
/// `[u8; NODE_SECRET_LEN]` here would silently strip both.
#[derive(Debug)]
pub struct InMemoryKeyStore {
    secret: NodeSecret,
}

impl InMemoryKeyStore {
    /// Creates a store returning `secret`.
    pub fn new(secret: NodeSecret) -> Self {
        Self { secret }
    }

    /// Creates a store returning a **fixed, publicly known** development secret.
    ///
    /// Every node built this way shares one identity. Use it only for local
    /// development and tests; never on a node reachable by peers.
    pub fn insecure_dev() -> Self {
        let mut bytes = [0u8; kamiroh_domain::secret::NODE_SECRET_LEN];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = i as u8;
        }
        Self {
            secret: NodeSecret::from_bytes(bytes),
        }
    }
}

#[async_trait]
impl KeyStore for InMemoryKeyStore {
    async fn load_or_create(&self) -> Result<NodeSecret, KeyStoreError> {
        Ok(self.secret.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_the_secret_it_was_given() {
        let store = InMemoryKeyStore::new(NodeSecret::from_bytes([3u8; 32]));
        let loaded = store.load_or_create().await.unwrap();
        assert_eq!(loaded.expose_bytes(), &[3u8; 32]);
    }

    #[test]
    fn debug_output_does_not_leak_the_secret() {
        // Holding a `NodeSecret` rather than raw bytes is what keeps this true.
        let store = InMemoryKeyStore::new(NodeSecret::from_bytes([0xab; 32]));
        let rendered = format!("{store:?}");
        assert!(rendered.contains("redacted"), "{rendered}");
        assert!(!rendered.contains("171"), "{rendered}");
        assert!(!rendered.contains("ab, ab"), "{rendered}");
    }

    #[tokio::test]
    async fn load_or_create_is_stable_across_calls() {
        let store = InMemoryKeyStore::insecure_dev();
        let first = store.load_or_create().await.unwrap();
        let second = store.load_or_create().await.unwrap();
        assert_eq!(first.expose_bytes(), second.expose_bytes());
    }
}
