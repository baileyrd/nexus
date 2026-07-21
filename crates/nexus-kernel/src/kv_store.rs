//! Key-value store trait for plugin state persistence.
//!
//! Each plugin gets an isolated namespace. The store is used by
//! `PluginContext::kv_get` / `kv_set` / `kv_delete` and by the hot-reload
//! system to preserve plugin state across reloads.
//!
//! The kernel defines the trait and a zero-dependency [`InMemoryKvStore`]
//! fake for tests; the real durable backend ([`SqliteKvStore`](../../nexus_kv/struct.SqliteKvStore.html))
//! lives in `nexus-kv`. Bootstrap picks a backend and passes it to
//! [`crate::Kernel::new`].

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::KvError;

// ─── KvStore trait ──────────────────────────────────────────────────────────

/// Abstract key-value storage backend for plugin state persistence.
///
/// Each namespace is isolated — plugins access only their own data.
/// Consumers (including `nexus-plugins`) interact through this trait; pick
/// a concrete impl from `nexus-kv` (e.g. `SqliteKvStore`, `InMemoryKvStore`).
pub trait KvStore: Send + Sync + std::fmt::Debug {
    /// Get a value by key within a namespace.
    ///
    /// # Errors
    /// Returns `KvError::BackendError` on storage failures.
    fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, KvError>;

    /// Set a value by key within a namespace (upsert).
    ///
    /// # Errors
    /// Returns `KvError::BackendError` on storage failures.
    fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), KvError>;

    /// Delete a key within a namespace. Returns `Ok(())` even if the key
    /// does not exist.
    ///
    /// # Errors
    /// Returns `KvError::BackendError` on storage failures.
    fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError>;

    /// List keys within a namespace whose name starts with `prefix` (an
    /// empty prefix lists every key in the namespace). Order is
    /// unspecified beyond being deterministic for a given backend.
    ///
    /// # Errors
    /// Returns `KvError::BackendError` on storage failures.
    fn list_keys(&self, namespace: &str, prefix: &str) -> Result<Vec<String>, KvError>;
}

/// Convenience constructor for `KvError::BackendError`.
impl KvError {
    /// Convert a plugin-crate `PluginError` style message into a `KvError`.
    #[must_use]
    pub fn backend(msg: impl Into<String>) -> Self {
        Self::BackendError { reason: msg.into() }
    }
}

// ─── InMemoryKvStore ────────────────────────────────────────────────────────

/// HashMap-backed KV store — zero-dependency fake for tests and embedding
/// scenarios that don't need durability.
///
/// Thread-safe via an internal `Mutex`. For the real on-disk backend, use
/// [`nexus_kv::SqliteKvStore`](../../nexus_kv/struct.SqliteKvStore.html).
#[derive(Debug, Default)]
pub struct InMemoryKvStore {
    inner: Mutex<HashMap<(String, String), Vec<u8>>>,
}

impl InMemoryKvStore {
    /// Construct an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl KvStore for InMemoryKvStore {
    fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, KvError> {
        Ok(self
            .inner
            .lock()
            .map_err(|e| KvError::backend(format!("lock poisoned: {e}")))?
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), KvError> {
        self.inner
            .lock()
            .map_err(|e| KvError::backend(format!("lock poisoned: {e}")))?
            .insert((namespace.to_string(), key.to_string()), value.to_vec());
        Ok(())
    }

    fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError> {
        self.inner
            .lock()
            .map_err(|e| KvError::backend(format!("lock poisoned: {e}")))?
            .remove(&(namespace.to_string(), key.to_string()));
        Ok(())
    }

    fn list_keys(&self, namespace: &str, prefix: &str) -> Result<Vec<String>, KvError> {
        let mut keys: Vec<String> = self
            .inner
            .lock()
            .map_err(|e| KvError::backend(format!("lock poisoned: {e}")))?
            .keys()
            .filter(|(ns, key)| ns == namespace && key.starts_with(prefix))
            .map(|(_, key)| key.clone())
            .collect();
        keys.sort_unstable();
        Ok(keys)
    }
}

#[cfg(test)]
mod in_memory_tests {
    use super::*;

    #[test]
    fn roundtrip_and_namespace_isolation() {
        let store = InMemoryKvStore::new();
        store.set("ns1", "k", b"a").unwrap();
        store.set("ns2", "k", b"b").unwrap();
        assert_eq!(store.get("ns1", "k").unwrap().unwrap(), b"a");
        assert_eq!(store.get("ns2", "k").unwrap().unwrap(), b"b");
        store.delete("ns1", "k").unwrap();
        assert!(store.get("ns1", "k").unwrap().is_none());
        assert_eq!(store.get("ns2", "k").unwrap().unwrap(), b"b");
    }

    #[test]
    fn list_keys_filters_by_namespace_and_prefix() {
        let store = InMemoryKvStore::new();
        store.set("ns1", "settings.theme", b"a").unwrap();
        store.set("ns1", "settings.font", b"b").unwrap();
        store.set("ns1", "cache.foo", b"c").unwrap();
        store.set("ns2", "settings.theme", b"d").unwrap();

        let mut keys = store.list_keys("ns1", "settings.").unwrap();
        keys.sort();
        assert_eq!(keys, vec!["settings.font", "settings.theme"]);

        let mut all_ns1 = store.list_keys("ns1", "").unwrap();
        all_ns1.sort();
        assert_eq!(all_ns1, vec!["cache.foo", "settings.font", "settings.theme"]);

        assert_eq!(store.list_keys("ns2", "cache.").unwrap(), Vec::<String>::new());
    }
}
