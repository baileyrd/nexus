//! On-disk SQLite-backed [`nexus_kernel::KvStore`] backend.
//!
//! `nexus-kernel` itself has no `SQLite` dependency; bootstrap picks a backend
//! and passes it to [`nexus_kernel::Kernel::new`]. Use [`SqliteKvStore`] for
//! the real runtime; for tests, use
//! [`nexus_kernel::InMemoryKvStore`](../nexus_kernel/struct.InMemoryKvStore.html).

#![deny(missing_docs)]
#![warn(clippy::pedantic)]

use std::path::Path;
use std::sync::Mutex;

use nexus_kernel::{KvError, KvStore};
use rusqlite::{Connection, OptionalExtension};

// ─── SqliteKvStore ──────────────────────────────────────────────────────────

/// SQLite-backed key-value store. Thread-safe via internal `Mutex`.
///
/// Schema: `kv_store(namespace TEXT, key TEXT, value BLOB, PRIMARY KEY(namespace, key))`
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
    /// Returns [`KvError::BackendError`] if the database cannot be opened or
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

    /// Open an in-memory SQLite-backed KV store (for testing the `SQLite`
    /// path specifically). Prefer [`InMemoryKvStore`] when you only need a
    /// fast fake.
    ///
    /// # Errors
    /// Returns [`KvError::BackendError`] if the in-memory database cannot be
    /// created.
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
}

impl KvStore for SqliteKvStore {
    fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, KvError> {
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

    fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), KvError> {
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

    fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError> {
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

    fn list_keys(&self, namespace: &str, prefix: &str) -> Result<Vec<String>, KvError> {
        let conn = self.conn.lock().map_err(|e| KvError::BackendError {
            reason: format!("lock poisoned: {e}"),
        })?;

        // Namespace stays an exact match (see the substring-collision tests
        // below) — only the key portion is a prefix search, and the
        // prefix's own LIKE metacharacters (`%`, `_`) are escaped so a key
        // prefix like "a_b" can't accidentally match "aXb".
        let escaped = escape_like_prefix(prefix);
        let pattern = format!("{escaped}%");

        let mut stmt = conn
            .prepare_cached(
                "SELECT key FROM kv_store WHERE namespace = ?1 AND key LIKE ?2 ESCAPE '\\' ORDER BY key",
            )
            .map_err(|e| KvError::BackendError {
                reason: format!("prepare failed: {e}"),
            })?;

        let keys = stmt
            .query_map(rusqlite::params![namespace, pattern], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| KvError::BackendError {
                reason: format!("query failed: {e}"),
            })?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(|e| KvError::BackendError {
                reason: format!("row read failed: {e}"),
            })?;

        Ok(keys)
    }
}

/// Escape `\`, `%`, and `_` so a caller-supplied prefix is matched literally
/// under `LIKE ... ESCAPE '\'` rather than as a wildcard pattern.
fn escape_like_prefix(prefix: &str) -> String {
    let mut escaped = String::with_capacity(prefix.len());
    for c in prefix.chars() {
        if matches!(c, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod sqlite_tests {
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

    /// Issue #85. The audit flagged that namespacing relies on
    /// string-equality with no test for cross-namespace
    /// interference on writes / deletes. The above
    /// `namespaces_are_isolated` covers `set`/`get`; this expands
    /// to delete + overwrite + plugin-id-substring shapes that are
    /// the obvious regression vectors for "string equality means
    /// `ns` and `ns2` could collide".
    #[test]
    fn delete_does_not_cross_namespaces() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("plugin.a", "session", b"alpha").unwrap();
        store.set("plugin.b", "session", b"beta").unwrap();

        store.delete("plugin.a", "session").unwrap();

        assert!(
            store.get("plugin.a", "session").unwrap().is_none(),
            "delete should remove the key from its own namespace"
        );
        assert_eq!(
            store.get("plugin.b", "session").unwrap().unwrap(),
            b"beta",
            "delete in plugin.a must not touch plugin.b's value"
        );
    }

    #[test]
    fn substring_namespaces_do_not_collide() {
        // `plugin` and `plugin.foo` share a string prefix. A naive
        // SQL `LIKE 'plugin%'` rather than `=` would collapse them.
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("plugin", "key", b"short").unwrap();
        store.set("plugin.foo", "key", b"long").unwrap();

        assert_eq!(store.get("plugin", "key").unwrap().unwrap(), b"short");
        assert_eq!(store.get("plugin.foo", "key").unwrap().unwrap(), b"long");

        store.delete("plugin", "key").unwrap();
        assert_eq!(
            store.get("plugin.foo", "key").unwrap().unwrap(),
            b"long",
            "deleting a prefix-namespace must not collateral-damage suffix namespaces"
        );
    }

    #[test]
    fn empty_namespace_is_distinct_from_others() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("", "key", b"empty-ns").unwrap();
        store.set("ns", "key", b"named-ns").unwrap();

        assert_eq!(store.get("", "key").unwrap().unwrap(), b"empty-ns");
        assert_eq!(store.get("ns", "key").unwrap().unwrap(), b"named-ns");
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

    #[test]
    fn list_keys_filters_by_prefix_within_a_namespace() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("ns", "settings.theme", b"a").unwrap();
        store.set("ns", "settings.font", b"b").unwrap();
        store.set("ns", "cache.foo", b"c").unwrap();

        let keys = store.list_keys("ns", "settings.").unwrap();
        assert_eq!(keys, vec!["settings.font", "settings.theme"]);

        let all = store.list_keys("ns", "").unwrap();
        assert_eq!(all, vec!["cache.foo", "settings.font", "settings.theme"]);
    }

    #[test]
    fn list_keys_does_not_cross_namespaces() {
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("plugin.a", "key1", b"a").unwrap();
        store.set("plugin.b", "key1", b"b").unwrap();

        assert_eq!(store.list_keys("plugin.a", "").unwrap(), vec!["key1"]);
        assert_eq!(store.list_keys("plugin.c", "").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn list_keys_escapes_like_metacharacters_in_the_prefix() {
        // A naive `LIKE prefix || '%'` would treat "a_b" as a wildcard
        // pattern (`_` matches any single char) and incorrectly match
        // "aXb". The escaping must make the literal prefix match only.
        let store = SqliteKvStore::in_memory().unwrap();
        store.set("ns", "a_b.exact", b"1").unwrap();
        store.set("ns", "aXb.should_not_match", b"2").unwrap();
        store.set("ns", "a%wild.should_not_match", b"3").unwrap();

        assert_eq!(store.list_keys("ns", "a_b").unwrap(), vec!["a_b.exact"]);
    }
}
