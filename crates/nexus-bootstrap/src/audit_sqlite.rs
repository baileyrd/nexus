//! SQLite-backed [`nexus_kernel::audit_store::AuditStore`] implementation.
//!
//! Lives here (not in `nexus-kernel`) because the kernel must remain
//! backend-agnostic — see `dep_invariants.rs` which forbids `rusqlite` in
//! `nexus-kernel`'s dependency graph. The kernel exposes the trait + free
//! functions; bootstrap installs an instance of this struct at boot.

use std::path::Path;
use std::sync::{Arc, Mutex};

use nexus_kernel::audit_store::{AuditEntry, AuditQuery, AuditStore};

const RETENTION_DAYS: i64 = nexus_types::constants::AUDIT_LOG_RETENTION_DAYS as i64;
const DEFAULT_QUERY_LIMIT: u32 = 1000;
const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS audit_events (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        ts_ms       INTEGER NOT NULL,
        event_type  TEXT    NOT NULL,
        plugin_id   TEXT,
        detail_json TEXT    NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_audit_ts          ON audit_events(ts_ms);
    CREATE INDEX IF NOT EXISTS idx_audit_event_type  ON audit_events(event_type);
    CREATE INDEX IF NOT EXISTS idx_audit_plugin      ON audit_events(plugin_id);
";

pub struct SqliteAuditStore {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteAuditStore {
    /// Open the audit database at `path`, creating it (and any missing
    /// parents) if absent. Runs the schema migration and prunes entries
    /// older than [`RETENTION_DAYS`].
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
                        Some(e.to_string()),
                    )
                })?;
            }
        }
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let cutoff = chrono::Utc::now().timestamp_millis() - RETENTION_DAYS * 86_400 * 1_000;
        let _ = conn.execute(
            "DELETE FROM audit_events WHERE ts_ms < ?1",
            rusqlite::params![cutoff],
        );
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl AuditStore for SqliteAuditStore {
    fn append(&self, event_type: &str, plugin_id: Option<&str>, detail: &serde_json::Value) {
        let ts_ms = chrono::Utc::now().timestamp_millis();
        let detail_str = detail.to_string();
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "audit store mutex poisoned; dropping event");
                return;
            }
        };
        if let Err(e) = conn.execute(
            "INSERT INTO audit_events (ts_ms, event_type, plugin_id, detail_json) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![ts_ms, event_type, plugin_id, detail_str],
        ) {
            tracing::warn!(error = %e, event_type, "audit append failed");
        }
    }

    fn query(&self, q: &AuditQuery) -> Vec<AuditEntry> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "audit store mutex poisoned");
                return Vec::new();
            }
        };
        let limit = i64::from(q.limit.unwrap_or(DEFAULT_QUERY_LIMIT));
        let result: Result<Vec<AuditEntry>, rusqlite::Error> = (|| {
            let mut stmt = conn.prepare(
                "SELECT id, ts_ms, event_type, plugin_id, detail_json
                 FROM audit_events
                 WHERE (?1 IS NULL OR event_type = ?1)
                   AND (?2 IS NULL OR plugin_id  = ?2)
                   AND (?3 IS NULL OR ts_ms     >= ?3)
                 ORDER BY ts_ms DESC
                 LIMIT ?4",
            )?;
            let rows = stmt.query_map(
                rusqlite::params![q.event_type, q.plugin_id, q.since_ts, limit],
                |row| {
                    Ok(AuditEntry {
                        id: row.get(0)?,
                        ts_ms: row.get(1)?,
                        event_type: row.get(2)?,
                        plugin_id: row.get(3)?,
                        detail_json: row.get(4)?,
                    })
                },
            )?;
            rows.collect()
        })();
        match result {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "audit query failed");
                Vec::new()
            }
        }
    }

    fn clear(&self, before_ts: i64) -> u64 {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "audit store mutex poisoned");
                return 0;
            }
        };
        match conn.execute(
            "DELETE FROM audit_events WHERE ts_ms < ?1",
            rusqlite::params![before_ts],
        ) {
            Ok(n) => n as u64,
            Err(e) => {
                tracing::warn!(error = %e, "audit clear failed");
                0
            }
        }
    }
}

/// Open a SQLite audit store at `path` and install it as the kernel's
/// global audit store. Subsequent `nexus_kernel::audit::log_*` calls will
/// persist to this file.
pub fn init(path: &Path) -> rusqlite::Result<()> {
    let store = SqliteAuditStore::open(path)?;
    nexus_kernel::audit_store::install(Arc::new(store));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, SqliteAuditStore) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("audit.db");
        let store = SqliteAuditStore::open(&path).unwrap();
        (dir, store)
    }

    #[test]
    fn open_creates_missing_parent_dir() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("a").join("audit.db");
        SqliteAuditStore::open(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn append_then_query_round_trips() {
        let (_dir, store) = temp_store();
        store.append(
            "capability_granted",
            Some("nexus.test"),
            &json!({"capability": "FsRead"}),
        );
        store.append(
            "capability_denied",
            Some("nexus.test"),
            &json!({"capability": "Net"}),
        );
        let all = store.query(&AuditQuery::default());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].event_type, "capability_denied");
        assert_eq!(all[1].event_type, "capability_granted");
    }

    #[test]
    fn query_filters_by_event_type() {
        let (_dir, store) = temp_store();
        store.append("capability_granted", Some("a"), &json!({}));
        store.append("capability_denied", Some("b"), &json!({}));
        let granted = store.query(&AuditQuery {
            event_type: Some("capability_granted".to_string()),
            ..Default::default()
        });
        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].plugin_id.as_deref(), Some("a"));
    }

    #[test]
    fn query_filters_by_plugin_id() {
        let (_dir, store) = temp_store();
        store.append("x", Some("a"), &json!({}));
        store.append("x", Some("b"), &json!({}));
        let a = store.query(&AuditQuery {
            plugin_id: Some("a".to_string()),
            ..Default::default()
        });
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].plugin_id.as_deref(), Some("a"));
    }

    #[test]
    fn query_respects_limit() {
        let (_dir, store) = temp_store();
        for i in 0..5 {
            store.append("x", Some("a"), &json!({"i": i}));
        }
        let limited = store.query(&AuditQuery {
            limit: Some(2),
            ..Default::default()
        });
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn clear_removes_entries_below_cutoff_only() {
        let (_dir, store) = temp_store();
        for _ in 0..3 {
            store.append("x", Some("a"), &json!({}));
        }
        // ts_ms cutoff = now+1h ⇒ everything is "older" and gets removed.
        let future = chrono::Utc::now().timestamp_millis() + 3_600_000;
        let removed = store.clear(future);
        assert_eq!(removed, 3);
        assert!(store.query(&AuditQuery::default()).is_empty());

        // Re-append; cutoff in the past keeps everything.
        store.append("x", Some("a"), &json!({}));
        let past = 0;
        assert_eq!(store.clear(past), 0);
        assert_eq!(store.query(&AuditQuery::default()).len(), 1);
    }

    #[test]
    fn append_handles_null_plugin_id() {
        let (_dir, store) = temp_store();
        store.append("system_event", None, &json!({}));
        let rows = store.query(&AuditQuery::default());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].plugin_id.is_none());
    }
}
