//! Central sync hub for Nexus memory.
//!
//! A standalone HTTP server that many Nexus instances push their memories to
//! and pull each other's from — the durable convergence point behind
//! "memory shared across Nexus implementations". It mirrors the proven
//! `remind_me` hub wire protocol:
//!
//! - `GET  /health`      — unauthenticated liveness probe.
//! - `POST /sync/push`   — bearer-authed batch upsert; last-write-wins on
//!   `updated_at`; replies `{ accepted, processed_ids, failed }`.
//! - `GET  /sync/pull`   — bearer-authed keyset page of records newer than a
//!   `(since, since_id)` cursor, optionally excluding one node; replies
//!   `{ records, count }`.
//!
//! The hub is deliberately **schema-agnostic**: each record is stored by its
//! `id` with its `updated_at` (the LWW key + cursor), the authoring `node_id`,
//! the pushing `origin_node` (hub-only bookkeeping, never returned), and the
//! whole record as an opaque JSON `payload`. New memory fields therefore need
//! no hub change. Conflict resolution is last-write-wins on the canonical
//! ISO-8601-UTC `updated_at` string (lexically orderable). Auth is a single
//! shared `SYNC_SECRET` bearer token, constant-time compared; there is no node
//! registry (any `node_id` is accepted), matching `remind_me`.

#![warn(clippy::pedantic)]

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Epoch default for `since` when a client omits it (pull everything).
const EPOCH: &str = "1970-01-01T00:00:00+00:00";
/// Hard cap on a single pull page (matches `remind_me`).
const MAX_PULL_LIMIT: usize = 500;
/// Default pull page size when the client omits `limit`.
const DEFAULT_PULL_LIMIT: usize = 100;

/// Errors from the hub store layer.
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    /// Underlying `SQLite` error.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Connection-pool error.
    #[error("pool: {0}")]
    Pool(#[from] r2d2::Error),
}

/// Result alias for the hub store.
pub type Result<T> = std::result::Result<T, HubError>;

/// Schema applied on open. One generic, schema-agnostic table.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS records (
    id          TEXT PRIMARY KEY,
    updated_at  TEXT NOT NULL,
    node_id     TEXT,
    origin_node TEXT,
    payload     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_records_cursor ON records(updated_at, id);";

/// Durable convergence store backing the hub.
#[derive(Clone)]
pub struct HubStore {
    pool: Pool<SqliteConnectionManager>,
}

impl std::fmt::Debug for HubStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubStore").finish_non_exhaustive()
    }
}

fn init_conn(conn: &mut rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;\nPRAGMA busy_timeout=5000;")
}

impl HubStore {
    /// Open (creating if needed) a hub database at `path`.
    ///
    /// # Errors
    /// Returns an error if the pool can't be built or the schema can't apply.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let manager = SqliteConnectionManager::file(path).with_init(init_conn);
        let store = Self {
            pool: Pool::new(manager)?,
        };
        store.pool.get()?.execute_batch(SCHEMA)?;
        Ok(store)
    }

    /// Open an in-memory hub backed by a single shared connection (tests).
    ///
    /// # Errors
    /// Returns an error if the pool can't be built or the schema can't apply.
    pub fn open_in_memory() -> Result<Self> {
        let manager = SqliteConnectionManager::memory().with_init(init_conn);
        let store = Self {
            pool: Pool::builder().max_size(1).build(manager)?,
        };
        store.pool.get()?.execute_batch(SCHEMA)?;
        Ok(store)
    }

    /// Upsert a batch of records, last-write-wins on `updated_at`. `origin_node`
    /// is the pushing node (recorded for `exclude_node` filtering, never
    /// returned on pull). Returns the ids that were valid and handled — records
    /// lacking a string `id` or `updated_at` are skipped (reported as failed).
    ///
    /// # Errors
    /// Returns an error on a write failure.
    pub fn push(&self, origin_node: &str, records: &[Value]) -> Result<Vec<String>> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        let mut processed = Vec::new();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO records (id, updated_at, node_id, origin_node, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                     updated_at = excluded.updated_at,
                     node_id = excluded.node_id,
                     origin_node = excluded.origin_node,
                     payload = excluded.payload
                 WHERE excluded.updated_at > records.updated_at;",
            )?;
            for record in records {
                let (Some(id), Some(updated_at)) = (
                    record.get("id").and_then(Value::as_str),
                    record.get("updated_at").and_then(Value::as_str),
                ) else {
                    continue; // not a valid syncable record
                };
                let node_id = record.get("node_id").and_then(Value::as_str);
                let payload = record.to_string();
                stmt.execute(params![id, updated_at, node_id, origin_node, payload])?;
                processed.push(id.to_string());
            }
        }
        tx.commit()?;
        Ok(processed)
    }

    /// Pull a keyset page of records strictly after the `(since, since_id)`
    /// cursor, newest-cursor-last, optionally excluding records pushed by
    /// `exclude_node`. When `since_id` is `None`, a strict `updated_at > since`
    /// is used (first-page / legacy). Returns the opaque record payloads.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn pull(
        &self,
        since: &str,
        since_id: Option<&str>,
        exclude_node: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let limit = limit.clamp(1, MAX_PULL_LIMIT);
        let conn = self.pool.get()?;

        // Build the WHERE incrementally with positional params.
        let mut sql = String::from("SELECT payload FROM records WHERE ");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(sid) = since_id {
            sql.push_str("(updated_at > ?1 OR (updated_at = ?1 AND id > ?2))");
            args.push(Box::new(since.to_string()));
            args.push(Box::new(sid.to_string()));
        } else {
            sql.push_str("updated_at > ?1");
            args.push(Box::new(since.to_string()));
        }
        if let Some(node) = exclude_node {
            sql.push_str(&format!(
                " AND (origin_node IS NULL OR origin_node != ?{})",
                args.len() + 1
            ));
            args.push(Box::new(node.to_string()));
        }
        sql.push_str(&format!(" ORDER BY updated_at ASC, id ASC LIMIT {limit}"));

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(AsRef::as_ref).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let payload: String = row.get(0)?;
                Ok(payload)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|p| serde_json::from_str::<Value>(&p).ok())
            .collect())
    }

    /// Total stored records.
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn count(&self) -> Result<u64> {
        let conn = self.pool.get()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM records", [], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}

// ── HTTP wire types ────────────────────────────────────────────────────────

/// Body of `POST /sync/push`.
#[derive(Debug, Deserialize)]
pub struct PushRequest {
    /// The pushing node's id (recorded as `origin_node`).
    #[serde(default)]
    pub node_id: String,
    /// Records to upsert (opaque JSON objects carrying at least `id` +
    /// `updated_at`).
    #[serde(default)]
    pub records: Vec<Value>,
}

/// Reply for `POST /sync/push`.
#[derive(Debug, Serialize)]
pub struct PushResponse {
    /// Number of records accepted (valid and handled).
    pub accepted: usize,
    /// Ids accepted — the client marks exactly these sent.
    pub processed_ids: Vec<String>,
    /// Number of records skipped for missing `id`/`updated_at`.
    pub failed: usize,
}

/// Query params for `GET /sync/pull`.
#[derive(Debug, Deserialize)]
pub struct PullQuery {
    /// Keyset cursor timestamp (default epoch).
    pub since: Option<String>,
    /// Keyset cursor id (for boundary ties).
    pub since_id: Option<String>,
    /// Node whose own pushes should be excluded.
    pub exclude_node: Option<String>,
    /// Max records to return (clamped to 1..=500).
    pub limit: Option<usize>,
}

/// Reply for `GET /sync/pull`.
#[derive(Debug, Serialize)]
pub struct PullResponse {
    /// The record payloads, cursor order.
    pub records: Vec<Value>,
    /// `records.len()`, for convenience.
    pub count: usize,
}

/// Shared server state.
#[derive(Clone)]
pub struct AppState {
    /// The convergence store.
    pub store: HubStore,
    /// Shared bearer secret all clients must present.
    pub secret: Arc<String>,
}

/// Build the hub's axum router over `state`.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sync/push", post(push))
        .route("/sync/pull", get(pull))
        .with_state(state)
}

/// Serve the hub over `listener` until the task is dropped. A thin wrapper over
/// [`router`] + `axum::serve` so embedders and tests need not depend on axum.
///
/// # Errors
/// Propagates the server's I/O error, if any.
pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

/// Constant-time byte-equality so token checks don't leak length-prefix timing.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Verify the `Authorization: Bearer <secret>` header.
fn authorize(headers: &HeaderMap, secret: &str) -> std::result::Result<(), (StatusCode, String)> {
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match presented {
        Some(token) if ct_eq(token.as_bytes(), secret.as_bytes()) => Ok(()),
        _ => Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string())),
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    role: &'static str,
    records: u64,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        role: "hub",
        records: state.store.count().unwrap_or(0),
    })
}

async fn push(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PushRequest>,
) -> std::result::Result<Json<PushResponse>, (StatusCode, String)> {
    authorize(&headers, &state.secret)?;
    let total = req.records.len();
    let processed = state
        .store
        .push(&req.node_id, &req.records)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("push: {e}")))?;
    let accepted = processed.len();
    Ok(Json(PushResponse {
        accepted,
        processed_ids: processed,
        failed: total - accepted,
    }))
}

async fn pull(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PullQuery>,
) -> std::result::Result<Json<PullResponse>, (StatusCode, String)> {
    authorize(&headers, &state.secret)?;
    let since = q.since.unwrap_or_else(|| EPOCH.to_string());
    let records = state
        .store
        .pull(
            &since,
            q.since_id.as_deref(),
            q.exclude_node.as_deref(),
            q.limit.unwrap_or(DEFAULT_PULL_LIMIT),
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("pull: {e}")))?;
    let count = records.len();
    Ok(Json(PullResponse { records, count }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rec(id: &str, updated_at: &str, node: &str) -> Value {
        json!({ "id": id, "updated_at": updated_at, "node_id": node, "content": format!("c-{id}") })
    }

    #[test]
    fn push_then_pull_round_trips() {
        let store = HubStore::open_in_memory().unwrap();
        let processed = store
            .push(
                "node-a",
                &[rec("m1", "2026-01-01T00:00:00+00:00", "node-a")],
            )
            .unwrap();
        assert_eq!(processed, vec!["m1"]);
        let out = store.pull(EPOCH, None, None, 100).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["content"], "c-m1");
    }

    #[test]
    fn push_skips_records_without_id_or_updated_at() {
        let store = HubStore::open_in_memory().unwrap();
        let processed = store
            .push(
                "node-a",
                &[
                    rec("m1", "2026-01-01T00:00:00+00:00", "node-a"),
                    json!({ "content": "no id/ts" }),
                ],
            )
            .unwrap();
        assert_eq!(processed, vec!["m1"]); // the invalid one is skipped
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn last_write_wins_on_updated_at() {
        let store = HubStore::open_in_memory().unwrap();
        store
            .push("a", &[rec("m1", "2026-01-01T00:00:00+00:00", "a")])
            .unwrap();
        // Older update is ignored.
        store
            .push(
                "b",
                &[json!({ "id": "m1", "updated_at": "2025-06-01T00:00:00+00:00", "content": "stale" })],
            )
            .unwrap();
        let out = store.pull(EPOCH, None, None, 100).unwrap();
        assert_eq!(out[0]["content"], "c-m1");
        // Newer update wins.
        store
            .push(
                "b",
                &[json!({ "id": "m1", "updated_at": "2026-12-01T00:00:00+00:00", "content": "fresh" })],
            )
            .unwrap();
        let out = store.pull(EPOCH, None, None, 100).unwrap();
        assert_eq!(out[0]["content"], "fresh");
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn pull_keyset_cursor_paginates_without_skipping_ties() {
        let store = HubStore::open_in_memory().unwrap();
        // Three rows share a timestamp — the keyset must page by id, not skip.
        let ts = "2026-01-01T00:00:00+00:00";
        store
            .push(
                "a",
                &[rec("m1", ts, "a"), rec("m2", ts, "a"), rec("m3", ts, "a")],
            )
            .unwrap();
        let page1 = store.pull(EPOCH, None, None, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0]["id"], "m1");
        assert_eq!(page1[1]["id"], "m2");
        // Resume from the last seen (ts, "m2").
        let page2 = store.pull(ts, Some("m2"), None, 2).unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0]["id"], "m3");
    }

    #[test]
    fn pull_excludes_origin_node() {
        let store = HubStore::open_in_memory().unwrap();
        store
            .push("a", &[rec("m1", "2026-01-01T00:00:00+00:00", "a")])
            .unwrap();
        store
            .push("b", &[rec("m2", "2026-01-02T00:00:00+00:00", "b")])
            .unwrap();
        // Node "a" pulls everything except what it pushed.
        let out = store.pull(EPOCH, None, Some("a"), 100).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "m2");
    }

    #[test]
    fn ct_eq_matches_only_identical() {
        assert!(ct_eq(b"secret", b"secret"));
        assert!(!ct_eq(b"secret", b"secrEt"));
        assert!(!ct_eq(b"secret", b"secret-longer"));
    }
}
