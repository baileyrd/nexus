//! BL-136 / ADR 0029 — persistent notification inbox.
//!
//! A SQLite-backed history of every `Notification` that reaches the
//! router's fan-out step. Owned by [`crate::core_plugin::NotificationsCorePlugin`];
//! surfaced via four IPC handlers (`inbox_list`, `inbox_mark_read`,
//! `inbox_dismiss`, `inbox_stats`). The inbox is a derived store
//! under `<forge>/.forge/notifications/inbox.db` — rebuildable from
//! the `com.nexus.notifications.delivered` event stream modulo
//! user-state columns (see [`Inbox::rebuild_from_events`]).
//!
//! ## Schema
//!
//! ```sql
//! CREATE TABLE inbox (
//!   id            TEXT PRIMARY KEY,
//!   source        TEXT NOT NULL,
//!   severity      TEXT NOT NULL DEFAULT 'info',
//!   title         TEXT,
//!   body          TEXT NOT NULL,
//!   channels      TEXT NOT NULL DEFAULT '[]',
//!   ts            INTEGER NOT NULL,
//!   read_at       INTEGER,
//!   dismissed_at  INTEGER,
//!   payload_json  TEXT
//! );
//! ```
//!
//! See the ADR for the index set + retention semantics.

use std::path::Path;

use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::config::Severity;
use crate::Channel;

/// Default retention cap when `notifications.toml::[inbox].max_rows`
/// is unset.
pub const DEFAULT_MAX_ROWS: u32 = 1000;
/// Default retention cap when `notifications.toml::[inbox].max_age_days`
/// is unset.
pub const DEFAULT_MAX_AGE_DAYS: u32 = 30;

/// Errors the inbox surfaces. Wrapped at the IPC layer into the
/// standard `PluginError::ExecutionFailed` shape.
#[derive(Debug, thiserror::Error)]
pub enum InboxError {
    /// SQLite layer error (open, migrate, query, exec).
    #[error("inbox sqlite error: {0}")]
    Sql(String),
    /// Channel-array JSON encode/decode failure.
    #[error("inbox payload encoding error: {0}")]
    Encoding(String),
}

impl From<rusqlite::Error> for InboxError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sql(err.to_string())
    }
}

/// One row of the inbox table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct InboxEntry {
    /// Stable UUIDv4 generated at insert time.
    pub id: String,
    /// Source tag from the router path, or `"override"` for explicit-
    /// channel sends.
    pub source: String,
    /// Severity tag at dispatch time.
    pub severity: Severity,
    /// Optional title (may be `None` even when the caller supplied one
    /// and a transport defaulted to "Nexus" — the inbox preserves the
    /// caller's view).
    pub title: Option<String>,
    /// Notification body.
    pub body: String,
    /// Channels the router picked. Empty when every transport
    /// rejected (still inserted — the inbox is the source of "this
    /// fired" history).
    pub channels: Vec<Channel>,
    /// Insert time in Unix seconds.
    pub ts: i64,
    /// Set when the user marks the row read; `None` until.
    pub read_at: Option<i64>,
    /// Set when the user dismisses the row; `None` until.
    pub dismissed_at: Option<i64>,
    /// Caller-supplied JSON blob — used for `task_id` cross-links to
    /// the BL-134 observability panel. May be `None`.
    pub payload_json: Option<String>,
}

/// Args for [`Inbox::insert`]. Borrowed payload so the call site
/// doesn't have to clone strings up front.
pub struct NewEntry<'a> {
    /// Source tag.
    pub source: &'a str,
    /// Severity.
    pub severity: Severity,
    /// Title.
    pub title: Option<&'a str>,
    /// Body.
    pub body: &'a str,
    /// Channels routed to.
    pub channels: &'a [Channel],
    /// Optional payload JSON.
    pub payload_json: Option<&'a str>,
}

/// Filter shape passed to [`Inbox::list`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    /// Every row regardless of `read_at` / `dismissed_at`.
    #[default]
    All,
    /// Only rows where `read_at IS NULL AND dismissed_at IS NULL`.
    Unread,
    /// Only rows where `dismissed_at IS NOT NULL`.
    Dismissed,
}

/// Stats reply for `inbox_stats`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct InboxStats {
    /// Total non-dismissed rows in the inbox.
    pub total: u32,
    /// Rows with `read_at IS NULL AND dismissed_at IS NULL`.
    pub unread: u32,
    /// Per-source unread counts. Sources with zero unread are
    /// omitted.
    pub by_source: std::collections::BTreeMap<String, u32>,
}

/// SQLite-backed inbox store. One connection per plugin instance —
/// SQLite handles concurrent reads internally and dispatch_routed is
/// the single writer.
pub struct Inbox {
    conn: Mutex<Connection>,
    max_rows: u32,
    max_age_days: u32,
}

impl Inbox {
    fn with_conn<R>(
        &self,
        f: impl FnOnce(&Connection) -> Result<R, InboxError>,
    ) -> Result<R, InboxError> {
        let guard = self
            .conn
            .lock()
            .map_err(|e| InboxError::Sql(format!("inbox mutex poisoned: {e}")))?;
        f(&guard)
    }
}

impl Inbox {
    /// Open or create the inbox at `db_path`. Creates parent
    /// directories as needed.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on filesystem or SQLite migration
    /// failures.
    pub fn open(
        db_path: impl AsRef<Path>,
        max_rows: u32,
        max_age_days: u32,
    ) -> Result<Self, InboxError> {
        let path = db_path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| InboxError::Sql(e.to_string()))?;
        }
        let conn = Connection::open(path)?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            max_rows,
            max_age_days,
        })
    }

    /// Open a memory-only inbox. Used by unit tests.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite migration failure.
    pub fn in_memory(max_rows: u32, max_age_days: u32) -> Result<Self, InboxError> {
        let conn = Connection::open_in_memory()?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            max_rows,
            max_age_days,
        })
    }

    fn migrate(conn: &Connection) -> Result<(), InboxError> {
        conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS inbox (
                id            TEXT PRIMARY KEY,
                source        TEXT NOT NULL,
                severity      TEXT NOT NULL DEFAULT 'info',
                title         TEXT,
                body          TEXT NOT NULL,
                channels      TEXT NOT NULL DEFAULT '[]',
                ts            INTEGER NOT NULL,
                read_at       INTEGER,
                dismissed_at  INTEGER,
                payload_json  TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_inbox_ts ON inbox(ts DESC);
            CREATE INDEX IF NOT EXISTS idx_inbox_unread
                ON inbox(read_at) WHERE read_at IS NULL;
            CREATE INDEX IF NOT EXISTS idx_inbox_source ON inbox(source, ts DESC);
            ",
        )?;
        Ok(())
    }

    /// Insert a row, running the row-cap pass after the insert. The
    /// generated id is returned.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures and
    /// [`InboxError::Encoding`] if the channels list can't be
    /// JSON-encoded (should be infeasible — the enum is plain).
    pub fn insert(&self, entry: &NewEntry<'_>) -> Result<String, InboxError> {
        let id = uuid::Uuid::new_v4().to_string();
        let ts = unix_now();
        let channels_json = serde_json::to_string(&entry.channels)
            .map_err(|e| InboxError::Encoding(e.to_string()))?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO inbox (id, source, severity, title, body, channels, ts, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    entry.source,
                    entry.severity.as_str(),
                    entry.title,
                    entry.body,
                    channels_json,
                    ts,
                    entry.payload_json,
                ],
            )?;
            Ok(())
        })?;
        self.enforce_row_cap()?;
        Ok(id)
    }

    /// Remove rows past the configured row cap. Called after every
    /// insert. Public for the test harness.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn enforce_row_cap(&self) -> Result<u32, InboxError> {
        if self.max_rows == 0 {
            return Ok(0);
        }
        // Order by SQLite's implicit `rowid` rather than `ts` —
        // `ts` is per-second and multiple inserts in the same
        // second tie. `rowid` is monotonically assigned at INSERT
        // time so it always reflects insertion order.
        let deleted = self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM inbox WHERE id IN (
                     SELECT id FROM inbox ORDER BY rowid ASC
                     LIMIT MAX(0, (SELECT COUNT(*) FROM inbox) - ?1)
                 )",
                params![self.max_rows],
            )?)
        })?;
        Ok(u32::try_from(deleted).unwrap_or(0))
    }

    /// Remove rows older than `max_age_days`. Called on `on_start`
    /// (one-time at boot — too expensive per-insert).
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn enforce_age_cap(&self) -> Result<u32, InboxError> {
        if self.max_age_days == 0 {
            return Ok(0);
        }
        let cutoff = unix_now() - i64::from(self.max_age_days) * 86_400;
        let deleted = self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM inbox WHERE ts < ?1", params![cutoff])?)
        })?;
        Ok(u32::try_from(deleted).unwrap_or(0))
    }

    /// Mark a batch of ids read. Returns the number of rows actually
    /// flipped (a row that was already read is *not* re-stamped, so
    /// the count reflects only newly-read rows).
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn mark_read(&self, ids: &[String]) -> Result<u32, InboxError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let ts = unix_now();
        let sql = format!(
            "UPDATE inbox SET read_at = ?1 WHERE id IN ({placeholders}) AND read_at IS NULL"
        );
        let n = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let mut bound: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
            bound.push(&ts);
            for id in ids {
                bound.push(id);
            }
            Ok(stmt.execute(rusqlite::params_from_iter(bound.iter().copied()))?)
        })?;
        Ok(u32::try_from(n).unwrap_or(0))
    }

    /// Mark a batch of ids dismissed (and read — dismissal implies
    /// the row was seen).
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn dismiss(&self, ids: &[String]) -> Result<u32, InboxError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let ts = unix_now();
        let sql = format!(
            "UPDATE inbox SET dismissed_at = ?1, read_at = COALESCE(read_at, ?1)
             WHERE id IN ({placeholders}) AND dismissed_at IS NULL"
        );
        let n = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let mut bound: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
            bound.push(&ts);
            for id in ids {
                bound.push(id);
            }
            Ok(stmt.execute(rusqlite::params_from_iter(bound.iter().copied()))?)
        })?;
        Ok(u32::try_from(n).unwrap_or(0))
    }

    /// List inbox rows newest-first under the supplied filters.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn list(
        &self,
        since: Option<i64>,
        status: StatusFilter,
        source: Option<&str>,
        limit: u32,
    ) -> Result<Vec<InboxEntry>, InboxError> {
        let mut sql = String::from(
            "SELECT id, source, severity, title, body, channels, ts,
                    read_at, dismissed_at, payload_json
             FROM inbox WHERE 1=1",
        );
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(s) = since {
            sql.push_str(" AND ts >= ?");
            binds.push(Box::new(s));
        }
        match status {
            StatusFilter::All => {}
            StatusFilter::Unread => {
                sql.push_str(" AND read_at IS NULL AND dismissed_at IS NULL");
            }
            StatusFilter::Dismissed => sql.push_str(" AND dismissed_at IS NOT NULL"),
        }
        if let Some(src) = source {
            sql.push_str(" AND source = ?");
            binds.push(Box::new(src.to_string()));
        }
        sql.push_str(" ORDER BY ts DESC, id DESC LIMIT ?");
        binds.push(Box::new(i64::from(limit)));
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
            let mut rows = stmt.query(rusqlite::params_from_iter(bind_refs))?;
            let mut out = Vec::new();
            while let Some(r) = rows.next()? {
                out.push(row_to_entry(r)?);
            }
            Ok(out)
        })
    }

    /// Compute aggregate stats for the inbox panel header.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn stats(&self) -> Result<InboxStats, InboxError> {
        self.with_conn(|conn| {
            let total: i64 = conn.query_row(
                "SELECT COUNT(*) FROM inbox WHERE dismissed_at IS NULL",
                [],
                |r| r.get(0),
            )?;
            let unread: i64 = conn.query_row(
                "SELECT COUNT(*) FROM inbox WHERE read_at IS NULL AND dismissed_at IS NULL",
                [],
                |r| r.get(0),
            )?;
            let mut stmt = conn.prepare(
                "SELECT source, COUNT(*) FROM inbox
                 WHERE read_at IS NULL AND dismissed_at IS NULL
                 GROUP BY source",
            )?;
            let mut by_source = std::collections::BTreeMap::new();
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                let src: String = r.get(0)?;
                let count: i64 = r.get(1)?;
                by_source.insert(src, u32::try_from(count).unwrap_or(0));
            }
            Ok(InboxStats {
                total: u32::try_from(total).unwrap_or(0),
                unread: u32::try_from(unread).unwrap_or(0),
                by_source,
            })
        })
    }

    /// Direct id lookup. Used by tests; the IPC surface goes through
    /// [`Self::list`].
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn get(&self, id: &str) -> Result<Option<InboxEntry>, InboxError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, source, severity, title, body, channels, ts,
                        read_at, dismissed_at, payload_json
                 FROM inbox WHERE id = ?1",
            )?;
            Ok(stmt.query_row(params![id], row_to_entry).optional()?)
        })
    }

    /// Rebuild semantics — preserve `(id, read_at, dismissed_at)`
    /// across a teardown + replay. Returns the existing user-state
    /// rows so the caller can re-apply them after re-inserting from
    /// the event log. Truncates the table.
    ///
    /// The Phase-1 implementation ships this as a library function
    /// only — the CLI surface lands in Phase 2/3.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn snapshot_user_state(&self) -> Result<Vec<UserStateRow>, InboxError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, read_at, dismissed_at FROM inbox
                 WHERE read_at IS NOT NULL OR dismissed_at IS NOT NULL",
            )?;
            let mut rows = stmt.query([])?;
            let mut out: Vec<UserStateRow> = Vec::new();
            while let Some(r) = rows.next()? {
                out.push(UserStateRow {
                    id: r.get(0)?,
                    read_at: r.get(1)?,
                    dismissed_at: r.get(2)?,
                });
            }
            Ok(out)
        })
    }

    /// Truncate every row. Used by rebuild flows; not exposed via
    /// IPC.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn clear(&self) -> Result<(), InboxError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM inbox", [])?;
            Ok(())
        })
    }

    /// Re-apply the snapshot taken by [`Self::snapshot_user_state`]
    /// after re-inserting rows from the event log.
    ///
    /// # Errors
    /// Returns [`InboxError::Sql`] on SQLite failures.
    pub fn apply_user_state(&self, rows: &[UserStateRow]) -> Result<u32, InboxError> {
        self.with_conn(|conn| {
            let mut updated = 0u32;
            for row in rows {
                let n = conn.execute(
                    "UPDATE inbox SET read_at = ?1, dismissed_at = ?2 WHERE id = ?3",
                    params![row.read_at, row.dismissed_at, row.id],
                )?;
                updated += u32::try_from(n).unwrap_or(0);
            }
            Ok(updated)
        })
    }
}

/// User-state tuple preserved across rebuild.
#[derive(Debug, Clone)]
pub struct UserStateRow {
    /// Row id.
    pub id: String,
    /// `read_at` column.
    pub read_at: Option<i64>,
    /// `dismissed_at` column.
    pub dismissed_at: Option<i64>,
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<InboxEntry> {
    let id: String = row.get(0)?;
    let severity_str: String = row.get(2)?;
    let channels_json: String = row.get(5)?;
    let severity = match severity_str.as_str() {
        "debug" => Severity::Debug,
        "warn" => Severity::Warn,
        "error" => Severity::Error,
        _ => Severity::Info,
    };
    // Channels are stored as a JSON array string. Corruption (a hand-
    // edited DB, a schema-migration bug) shouldn't fail the whole
    // query — but it also shouldn't silently strip the channel set.
    // Log a warn so operators see corruption and the caller knows the
    // entry is degraded.
    let channels: Vec<Channel> = match serde_json::from_str(&channels_json) {
        Ok(channels) => channels,
        Err(e) => {
            tracing::warn!(
                entry_id = %id,
                channels_json = %channels_json,
                error = %e,
                "inbox entry has corrupt channels_json; returning entry with empty channel set"
            );
            Vec::new()
        }
    };
    Ok(InboxEntry {
        id,
        source: row.get(1)?,
        severity,
        title: row.get(3)?,
        body: row.get(4)?,
        channels,
        ts: row.get(6)?,
        read_at: row.get(7)?,
        dismissed_at: row.get(8)?,
        payload_json: row.get(9)?,
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(max_rows: u32) -> Inbox {
        Inbox::in_memory(max_rows, 0).expect("open in-memory inbox")
    }

    fn insert_one(i: &Inbox, src: &str, body: &str, ch: &[Channel]) -> String {
        i.insert(&NewEntry {
            source: src,
            severity: Severity::Info,
            title: Some("t"),
            body,
            channels: ch,
            payload_json: None,
        })
        .expect("insert")
    }

    #[test]
    fn insert_round_trips_through_get() {
        let i = fresh(100);
        let id = i
            .insert(&NewEntry {
                source: "workflow",
                severity: Severity::Warn,
                title: Some("Backup"),
                body: "done",
                channels: &[Channel::Desktop, Channel::Discord],
                payload_json: Some(r#"{"task_id":"abc"}"#),
            })
            .expect("insert");
        let got = i.get(&id).expect("get").expect("present");
        assert_eq!(got.source, "workflow");
        assert_eq!(got.severity, Severity::Warn);
        assert_eq!(got.title.as_deref(), Some("Backup"));
        assert_eq!(got.body, "done");
        assert_eq!(got.channels, vec![Channel::Desktop, Channel::Discord]);
        assert_eq!(got.payload_json.as_deref(), Some(r#"{"task_id":"abc"}"#));
        assert!(got.read_at.is_none());
    }

    #[test]
    fn mark_read_only_flips_unread_rows() {
        let i = fresh(100);
        let a = insert_one(&i, "wf", "a", &[]);
        let b = insert_one(&i, "wf", "b", &[]);
        let updated = i.mark_read(&[a.clone(), b.clone()]).expect("mark_read 1");
        assert_eq!(updated, 2);
        // Second call is a no-op — rows are already read.
        let updated = i.mark_read(&[a, b]).expect("mark_read 2");
        assert_eq!(updated, 0);
    }

    #[test]
    fn dismiss_also_marks_read() {
        let i = fresh(100);
        let id = insert_one(&i, "wf", "a", &[]);
        let n = i.dismiss(std::slice::from_ref(&id)).expect("dismiss");
        assert_eq!(n, 1);
        let got = i.get(&id).expect("get").expect("present");
        assert!(got.read_at.is_some());
        assert!(got.dismissed_at.is_some());
    }

    #[test]
    fn list_filters_unread() {
        let i = fresh(100);
        let a = insert_one(&i, "wf", "a", &[]);
        let _b = insert_one(&i, "wf", "b", &[]);
        i.mark_read(&[a]).expect("mark");
        let unread = i.list(None, StatusFilter::Unread, None, 10).expect("list");
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].body, "b");
    }

    #[test]
    fn list_filters_by_source() {
        let i = fresh(100);
        insert_one(&i, "wf", "a", &[]);
        insert_one(&i, "ai_runtime", "b", &[]);
        let only_wf = i
            .list(None, StatusFilter::All, Some("wf"), 10)
            .expect("list");
        assert_eq!(only_wf.len(), 1);
        assert_eq!(only_wf[0].source, "wf");
    }

    #[test]
    fn list_orders_newest_first() {
        let i = fresh(100);
        insert_one(&i, "wf", "first", &[]);
        std::thread::sleep(std::time::Duration::from_secs(1));
        insert_one(&i, "wf", "second", &[]);
        let rows = i.list(None, StatusFilter::All, None, 10).expect("list");
        assert_eq!(rows[0].body, "second");
        assert_eq!(rows[1].body, "first");
    }

    #[test]
    fn row_cap_trims_oldest_after_insert() {
        let i = fresh(3);
        let _a = insert_one(&i, "wf", "a", &[]);
        let _b = insert_one(&i, "wf", "b", &[]);
        let _c = insert_one(&i, "wf", "c", &[]);
        let _d = insert_one(&i, "wf", "d", &[]);
        let rows = i.list(None, StatusFilter::All, None, 10).expect("list");
        assert_eq!(rows.len(), 3);
        // Newest three survive — oldest ("a") drops.
        let bodies: Vec<_> = rows.iter().map(|r| r.body.as_str()).collect();
        assert!(!bodies.contains(&"a"));
    }

    #[test]
    fn row_cap_zero_disables_trim() {
        let i = fresh(0);
        for _ in 0..5 {
            insert_one(&i, "wf", "x", &[]);
        }
        let rows = i.list(None, StatusFilter::All, None, 100).expect("list");
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn stats_reflects_unread_and_per_source() {
        let i = fresh(100);
        let a = insert_one(&i, "wf", "a", &[]);
        insert_one(&i, "wf", "b", &[]);
        insert_one(&i, "ai_runtime", "c", &[]);
        i.mark_read(&[a]).expect("mark");
        let s = i.stats().expect("stats");
        assert_eq!(s.total, 3);
        assert_eq!(s.unread, 2);
        assert_eq!(s.by_source.get("wf").copied(), Some(1));
        assert_eq!(s.by_source.get("ai_runtime").copied(), Some(1));
    }

    #[test]
    fn user_state_snapshot_round_trip_preserves_read_dismissed() {
        let i = fresh(100);
        let a = insert_one(&i, "wf", "a", &[]);
        let b = insert_one(&i, "wf", "b", &[]);
        let _c = insert_one(&i, "wf", "c", &[]);
        i.mark_read(std::slice::from_ref(&a)).expect("mark");
        i.dismiss(std::slice::from_ref(&b)).expect("dismiss");

        let snap = i.snapshot_user_state().expect("snap");
        assert_eq!(snap.len(), 2);

        // Simulate a rebuild — drop every row, re-insert (with the
        // same ids), and re-apply the user state.
        let preserved: Vec<_> = i
            .list(None, StatusFilter::All, None, 100)
            .expect("list")
            .into_iter()
            .map(|e| (e.id, e.source, e.body, e.channels))
            .collect();
        i.clear().expect("clear");
        for (id, source, body, channels) in &preserved {
            let channels_json = serde_json::to_string(channels).unwrap();
            i.with_conn(|conn| {
                conn.execute(
                    "INSERT INTO inbox (id, source, severity, body, channels, ts)
                     VALUES (?1, ?2, 'info', ?3, ?4, ?5)",
                    params![id, source, body, channels_json, unix_now()],
                )?;
                Ok(())
            })
            .unwrap();
        }
        let applied = i.apply_user_state(&snap).expect("apply");
        assert_eq!(applied, 2);

        let after_a = i.get(&a).expect("get a").expect("present");
        let after_b = i.get(&b).expect("get b").expect("present");
        assert!(after_a.read_at.is_some(), "read_at preserved for a");
        assert!(
            after_b.dismissed_at.is_some(),
            "dismissed_at preserved for b"
        );
    }

    #[test]
    fn channels_array_round_trips_through_json_column() {
        let i = fresh(100);
        let id = i
            .insert(&NewEntry {
                source: "wf",
                severity: Severity::Info,
                title: None,
                body: "x",
                channels: &[Channel::Desktop, Channel::Telegram, Channel::Email],
                payload_json: None,
            })
            .expect("insert");
        let got = i.get(&id).expect("get").expect("present");
        assert_eq!(
            got.channels,
            vec![Channel::Desktop, Channel::Telegram, Channel::Email]
        );
    }

    #[test]
    fn empty_channels_means_routed_zero_transports() {
        let i = fresh(100);
        let id = insert_one(&i, "wf", "no-route", &[]);
        let got = i.get(&id).expect("get").expect("present");
        assert!(got.channels.is_empty());
    }
}
