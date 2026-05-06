//! Persistence trait + global access for audit events.
//!
//! BL-094 — backs the [`crate::audit`] emission helpers with a pluggable
//! store. The kernel is **backend-agnostic** (microkernel invariant
//! enforced by `dep_invariants.rs`); the SQLite implementation lives in
//! `nexus-bootstrap` and is installed via [`install`] at boot.
//!
//! Emission is **infallible** from the caller's perspective. Database
//! failures are logged via `tracing::warn!` by the implementation and
//! swallowed — audit pipelines must never break the operation they record.

use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

/// One audit event row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Auto-increment row id.
    pub id: i64,
    /// Unix milliseconds at insertion.
    pub ts_ms: i64,
    /// Event type discriminator (e.g. `"capability_granted"`).
    pub event_type: String,
    /// Plugin id the event refers to, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Event-specific JSON payload (raw string — caller parses).
    pub detail_json: String,
}

/// Filter for [`AuditStore::query`] / [`query`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditQuery {
    /// Restrict to this event type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    /// Restrict to this plugin id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Only entries with `ts_ms >= since_ts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ts: Option<i64>,
    /// Cap on returned rows (default depends on backend; nominally 1000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Backend trait. The SQLite implementation lives in `nexus-bootstrap`.
pub trait AuditStore: Send + Sync {
    /// Append one event. Best-effort — implementations should swallow
    /// errors and emit a `tracing::warn!`.
    fn append(&self, event_type: &str, plugin_id: Option<&str>, detail: &serde_json::Value);

    /// Query stored events in reverse chronological order (newest first).
    fn query(&self, q: &AuditQuery) -> Vec<AuditEntry>;

    /// Delete entries with `ts_ms < before_ts`. Returns the number of
    /// rows removed. Best-effort: implementations log and return 0 on
    /// failure rather than propagating errors.
    fn clear(&self, before_ts: i64) -> u64;
}

// ── Global access ─────────────────────────────────────────────────────────────

static AUDIT_STORE: OnceLock<Arc<dyn AuditStore>> = OnceLock::new();

/// Install a global audit store. Idempotent — second and subsequent calls
/// are silently dropped (matches `OnceLock` semantics).
pub fn install(store: Arc<dyn AuditStore>) {
    let _ = AUDIT_STORE.set(store);
}

/// Append an event to the global store, if one is installed.
/// No-op when audit persistence is disabled.
pub fn append(event_type: &str, plugin_id: Option<&str>, detail: &serde_json::Value) {
    if let Some(s) = AUDIT_STORE.get() {
        s.append(event_type, plugin_id, detail);
    }
}

/// Query the global store. Returns an empty vec if no store is installed.
#[must_use]
pub fn query(filter: &AuditQuery) -> Vec<AuditEntry> {
    AUDIT_STORE
        .get()
        .map(|s| s.query(filter))
        .unwrap_or_default()
}

/// Delete entries older than `before_ts` from the global store. Returns
/// the number of rows removed, or 0 if no store is installed.
pub fn clear(before_ts: i64) -> u64 {
    AUDIT_STORE
        .get()
        .map(|s| s.clear(before_ts))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// In-memory test fake — proves the trait shape and that the global
    /// install/append/query path works without dragging SQLite into the
    /// kernel's test surface.
    struct FakeStore {
        events: Mutex<Vec<AuditEntry>>,
    }

    impl AuditStore for FakeStore {
        fn append(&self, event_type: &str, plugin_id: Option<&str>, detail: &serde_json::Value) {
            let mut g = self.events.lock().unwrap();
            let id = g.len() as i64 + 1;
            g.push(AuditEntry {
                id,
                ts_ms: id, // monotonic stand-in for the test
                event_type: event_type.to_string(),
                plugin_id: plugin_id.map(str::to_string),
                detail_json: detail.to_string(),
            });
        }

        fn query(&self, q: &AuditQuery) -> Vec<AuditEntry> {
            let g = self.events.lock().unwrap();
            let mut rows: Vec<AuditEntry> = g
                .iter()
                .filter(|e| q.event_type.as_deref().is_none_or(|t| e.event_type == t))
                .filter(|e| q.plugin_id.as_deref().is_none_or(|p| e.plugin_id.as_deref() == Some(p)))
                .filter(|e| q.since_ts.is_none_or(|t| e.ts_ms >= t))
                .cloned()
                .collect();
            rows.reverse(); // newest first
            if let Some(limit) = q.limit {
                rows.truncate(limit as usize);
            }
            rows
        }

        fn clear(&self, before_ts: i64) -> u64 {
            let mut g = self.events.lock().unwrap();
            let before = g.len();
            g.retain(|e| e.ts_ms >= before_ts);
            (before - g.len()) as u64
        }
    }

    #[test]
    fn append_then_query_round_trips_through_fake() {
        let store = FakeStore { events: Mutex::new(Vec::new()) };
        store.append("capability_granted", Some("nexus.test"), &json!({"capability": "FsRead"}));
        store.append("capability_denied",  Some("nexus.test"), &json!({"capability": "Net"}));
        let all = store.query(&AuditQuery::default());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].event_type, "capability_denied");
        assert_eq!(all[1].event_type, "capability_granted");
    }

    #[test]
    fn fake_query_filters_by_event_type() {
        let store = FakeStore { events: Mutex::new(Vec::new()) };
        store.append("capability_granted", Some("a"), &json!({}));
        store.append("capability_denied", Some("b"), &json!({}));
        let granted = store.query(&AuditQuery {
            event_type: Some("capability_granted".to_string()),
            ..Default::default()
        });
        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].plugin_id.as_deref(), Some("a"));
    }
}
