//! SQLite-backed key-value store for plugin state persistence.
//!
//! Each plugin gets an isolated namespace. The store is used by
//! `PluginContext::kv_get` / `kv_set` / `kv_delete` and by the hot-reload
//! system to preserve plugin state across reloads.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};

use crate::error::KvError;

/// SQLite-backed key-value store. Thread-safe via internal `Mutex`.
///
/// Schema: `kv_store(namespace TEXT, key TEXT, value BLOB, PRIMARY KEY(namespace, key))`
///
/// Plugins interact with this through `PluginContext`; the namespace is
/// always the plugin's id, enforced by the context impl.
pub struct SqliteKvStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SqliteKvStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteKvStore").finish_non_exhaustive()
    }
}

impl SqliteKvStore {
    /// Open (or create) the KV store at the given path.
    ///
    /// # Errors
    /// Returns `KvError::BackendError` if the database cannot be opened or
    /// the schema migration fails.
    pub fn open(path: &Path) -> Result<Self, KvError> {
        let conn = Connection::open(path).map_err(|e| KvError::BackendError {
            reason: format!("failed to open KV database at {}: {e}", path.display()),
        })?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS kv_store (
                 namespace TEXT NOT NULL,
                 key       TEXT NOT NULL,
                 value     BLOB NOT NULL,
                 PRIMARY KEY (namespace, key)
             );",
        )
        .map_err(|e| KvError::BackendError {
            reason: format!("KV schema migration failed: {e}"),
        })?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory KV store (for testing).
    ///
    /// # Errors
    /// Returns `KvError::BackendError` if the in-memory database cannot be
    /// created.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self, KvError> {
        let conn = Connection::open_in_memory().map_err(|e| KvError::BackendError {
            reason: format!("failed to open in-memory KV database: {e}"),
        })?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kv_store (
                 namespace TEXT NOT NULL,
                 key       TEXT NOT NULL,
                 value     BLOB NOT NULL,
                 PRIMARY KEY (namespace, key)
             );",
        )
        .map_err(|e| KvError::BackendError {
            reason: format!("KV schema migration failed: {e}"),
        })?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Get a value by key within a namespace.
    ///
    /// # Errors
    /// Returns `KvError::BackendError` on SQLite failures.
    pub fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, KvError> {
        let conn = self.conn.lock().map_err(|e| KvError::BackendError {
            reason: format!("lock poisoned: {e}"),
        })?;

        let mut stmt = conn
            .prepare_cached("SELECT value FROM kv_store WHERE namespace = ?1 AND key = ?2")
            .map_err(|e| KvError::BackendError {
                reason: format!("prepare failed: {e}"),
            })?;

        let result = stmt
            .query_row(rusqlite::params![namespace, key], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .optional()
            .map_err(|e| KvError::BackendError {
                reason: format!("query failed: {e}"),
            })?;

        Ok(result)
    }

    /// Set a value by key within a namespace (upsert).
    ///
    /// # Errors
    /// Returns `KvError::BackendError` on SQLite failures.
    pub fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), KvError> {
        let conn = self.conn.lock().map_err(|e| KvError::BackendError {
            reason: format!("lock poisoned: {e}"),
        })?;

        conn.execute(
            "INSERT INTO kv_store (namespace, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(namespace, key) DO UPDATE SET value = excluded.value",
            rusqlite::params![namespace, key, value],
        )
        .map_err(|e| KvError::BackendError {
            reason: format!("upsert failed: {e}"),
        })?;

        Ok(())
    }

    /// Delete a key within a namespace. Returns `Ok(())` even if the key
    /// does not exist.
    ///
    /// # Errors
    /// Returns `KvError::BackendError` on SQLite failures.
    pub fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError> {
        let conn = self.conn.lock().map_err(|e| KvError::BackendError {
            reason: format!("lock poisoned: {e}"),
        })?;

        conn.execute(
            "DELETE FROM kv_store WHERE namespace = ?1 AND key = ?2",
            rusqlite::params![namespace, key],
        )
        .map_err(|e| KvError::BackendError {
            reason: format!("delete failed: {e}"),
        })?;

        Ok(())
    }
}

/// Trait alias so `nexus-plugins` can depend on the interface without
/// pulling in `rusqlite`.  Mirrors the `KvStore` trait in `nexus-plugins`
/// but lives in the kernel crate for ownership reasons.
impl crate::error::KvError {
    /// Convert a plugin-crate `PluginError` style message into a `KvError`.
    #[must_use]
    pub fn backend(msg: impl Into<String>) -> Self {
        Self::BackendError {
            reason: msg.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_nonexistent_returns_none() {
        let store = SqliteKvStore::in_memory().unwrap();
        let result = store.get("ns", "missing").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn set_and_get_roundtrip() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("ns", "key1", b"hello").unwrap();
        let val = store.get("ns", "key1").unwrap().unwrap();
        assert_eq!(val, b"hello");
    }

    #[test]
    fn set_overwrites_existing() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("ns", "key1", b"first").unwrap();
        store.set("ns", "key1", b"second").unwrap();
        let val = store.get("ns", "key1").unwrap().unwrap();
        assert_eq!(val, b"second");
    }

    #[test]
    fn namespaces_are_isolated() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("ns1", "key", b"val1").unwrap();
        store.set("ns2", "key", b"val2").unwrap();

        assert_eq!(store.get("ns1", "key").unwrap().unwrap(), b"val1");
        assert_eq!(store.get("ns2", "key").unwrap().unwrap(), b"val2");
    }

    #[test]
    fn delete_removes_key() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("ns", "key", b"val").unwrap();
        store.delete("ns", "key").unwrap();
        assert!(store.get("ns", "key").unwrap().is_none());
    }

    #[test]
    fn delete_nonexistent_is_ok() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.delete("ns", "missing").unwrap();
    }

    #[test]
    fn binary_data_roundtrips() {
        let store = SqliteKvStore::in_memory().unwrap();
        let binary: Vec<u8> = (0..=255).collect();
        store.set("ns", "bin", &binary).unwrap();
        let val = store.get("ns", "bin").unwrap().unwrap();
        assert_eq!(val, binary);
    }
}
