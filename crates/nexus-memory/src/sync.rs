//! Client sync engine — push/pull against a `nexus-memory-hub`.
//!
//! Mirrors the hub's wire protocol (see `nexus-memory-hub`): the client pushes
//! its memories newer than a keyset cursor to `POST /sync/push`, and pulls
//! everyone else's from `GET /sync/pull` (excluding its own node), applying each
//! with last-write-wins ([`MemoryDb::upsert_lww`]). Cursors persist in
//! `sync_state` so each run resumes where it left off. Deletes are local-only
//! (no tombstones), matching the hub.
//!
//! Config (`hub_url`, `secret`, `node_id`) is supplied per call by the caller
//! (CLI/MCP/shell/scheduler), so this stays decoupled from any config file.

use std::time::Duration;

use serde_json::{json, Value};

use crate::db::MemoryDb;
use crate::model::Memory;

/// Per-request HTTP timeout.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Records per push/pull page.
const BATCH: usize = 200;
/// Epoch cursor — "everything" when no cursor is stored yet.
const EPOCH: &str = "1970-01-01T00:00:00+00:00";

const PUSH_TS: &str = "sync.push.updated_at";
const PUSH_ID: &str = "sync.push.id";
const PULL_TS: &str = "sync.pull.updated_at";
const PULL_ID: &str = "sync.pull.id";

/// Resolved hub connection config.
struct HubConfig {
    url: String,
    secret: String,
    node_id: String,
}

fn parse_config(args: &Value) -> Result<HubConfig, String> {
    let field = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("sync: missing '{k}'"))
    };
    Ok(HubConfig {
        url: field("hub_url")?.trim_end_matches('/').to_string(),
        secret: field("secret")?,
        node_id: field("node_id")?,
    })
}

/// Run one full sync cycle (push then pull) against the hub in `args`.
/// Returns `{ pushed, pulled }`.
pub(crate) async fn sync(db: MemoryDb, args: &Value) -> Result<Value, String> {
    let cfg = parse_config(args)?;
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("sync: http client: {e}"))?;
    let pushed = push(&db, &client, &cfg).await?;
    let pulled = pull(&db, &client, &cfg).await?;
    Ok(json!({ "pushed": pushed, "pulled": pulled }))
}

/// Push local memories newer than the stored push cursor, advancing it.
///
/// Only memories authored here (`node_id` unset or equal to our node) are sent,
/// and each is stamped with our `node_id` so other nodes record us as the
/// author — that author stamp is what lets every node skip re-pushing memories
/// it merely pulled (no echo). The cursor still advances over the whole scanned
/// page, so foreign rows are seen once and never re-scanned.
///
/// v1 limitation: edits *here* to a memory authored *elsewhere* are not pushed
/// (its `node_id` stays foreign); whole-store authorship-agnostic sync would
/// need an explicit change outbox.
async fn push(db: &MemoryDb, client: &reqwest::Client, cfg: &HubConfig) -> Result<u64, String> {
    let mut ts = db.sync_state_get(PUSH_TS).map_err(de)?.unwrap_or_else(|| EPOCH.to_string());
    let mut id = db.sync_state_get(PUSH_ID).map_err(de)?.unwrap_or_default();
    let mut total = 0_u64;
    loop {
        let batch = db.list_since(&ts, &id, BATCH).map_err(de)?;
        if batch.is_empty() {
            break;
        }
        let records: Vec<Value> = batch
            .iter()
            .filter(|m| m.node_id.as_deref().is_none_or(|n| n == cfg.node_id))
            .filter_map(|m| {
                let mut v = serde_json::to_value(m).ok()?;
                v.as_object_mut()?
                    .insert("node_id".to_string(), json!(cfg.node_id));
                Some(v)
            })
            .collect();
        if !records.is_empty() {
            let sent = records.len() as u64;
            let resp = client
                .post(format!("{}/sync/push", cfg.url))
                .bearer_auth(&cfg.secret)
                .json(&json!({ "node_id": cfg.node_id, "records": records }))
                .send()
                .await
                .map_err(|e| format!("sync push: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("sync push: HTTP {}", resp.status()));
            }
            total += sent;
        }
        // Advance the cursor over the whole scanned page (keyset order), so
        // skipped foreign rows are not re-scanned next time.
        if let Some(last) = batch.last() {
            ts = last.updated_at.to_rfc3339();
            id = last.id.to_string();
            db.sync_state_set(PUSH_TS, &ts).map_err(de)?;
            db.sync_state_set(PUSH_ID, &id).map_err(de)?;
        }
        if batch.len() < BATCH {
            break;
        }
    }
    Ok(total)
}

/// Pull remote memories newer than the stored pull cursor, applying each with
/// last-write-wins and advancing the cursor.
async fn pull(db: &MemoryDb, client: &reqwest::Client, cfg: &HubConfig) -> Result<u64, String> {
    let mut ts = db.sync_state_get(PULL_TS).map_err(de)?.unwrap_or_else(|| EPOCH.to_string());
    let mut id = db.sync_state_get(PULL_ID).map_err(de)?;
    let mut total = 0_u64;
    loop {
        let mut req = client
            .get(format!("{}/sync/pull", cfg.url))
            .bearer_auth(&cfg.secret)
            .query(&[
                ("since", ts.as_str()),
                ("exclude_node", cfg.node_id.as_str()),
                ("limit", "200"),
            ]);
        if let Some(sid) = id.as_deref() {
            req = req.query(&[("since_id", sid)]);
        }
        let resp = req.send().await.map_err(|e| format!("sync pull: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("sync pull: HTTP {}", resp.status()));
        }
        let body: Value = resp.json().await.map_err(|e| format!("sync pull: decode: {e}"))?;
        let records = body.get("records").and_then(Value::as_array).cloned().unwrap_or_default();
        if records.is_empty() {
            break;
        }
        // Advance the cursor across the whole page (even records we can't decode,
        // so a bad row never wedges the cursor), and apply the decodable ones.
        for rec in &records {
            if let Some(u) = rec.get("updated_at").and_then(Value::as_str) {
                ts = u.to_string();
            }
            if let Some(i) = rec.get("id").and_then(Value::as_str) {
                id = Some(i.to_string());
            }
            if let Ok(m) = serde_json::from_value::<Memory>(rec.clone()) {
                let _ = db.upsert_lww(&m).map_err(de)?;
            }
        }
        db.sync_state_set(PULL_TS, &ts).map_err(de)?;
        db.sync_state_set(PULL_ID, id.as_deref().unwrap_or("")).map_err(de)?;
        total += records.len() as u64;
        if records.len() < BATCH {
            break;
        }
    }
    Ok(total)
}

/// Map a db error to the engine's `String` error.
fn de(e: crate::db::MemoryDbError) -> String {
    format!("sync: db: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_requires_hub_config() {
        let db = MemoryDb::open_in_memory().unwrap();
        let err = sync(db, &json!({ "node_id": "a", "secret": "s" })).await.unwrap_err();
        assert!(err.contains("missing 'hub_url'"), "got: {err}");
    }

    #[test]
    fn parse_config_trims_trailing_slash() {
        let cfg = parse_config(&json!({
            "hub_url": "http://host:8765/",
            "secret": "s",
            "node_id": "n"
        }))
        .unwrap();
        assert_eq!(cfg.url, "http://host:8765");
    }
}
