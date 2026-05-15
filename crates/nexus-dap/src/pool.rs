//! Connection pool for DAP clients keyed by adapter name.
//!
//! Mirrors [`nexus-lsp::ConnectionPool`](../nexus_lsp/struct.ConnectionPool.html):
//!
//! - **Lazy connect** — first call to [`ConnectionPool::get_or_connect`]
//!   spawns the child process; subsequent calls reuse the live entry.
//! - **Reconnect with backoff** — [`ConnectionPool::call_with_reconnect`]
//!   wraps an op closure; transient failures trigger a reconnect and
//!   retry against the configured backoff schedule.
//! - **Breakpoint replay** — when reconnecting, the cached
//!   per-source breakpoint set is replayed against the fresh adapter
//!   so the user's pins survive an adapter crash.
//! - **Shutdown all** — [`ConnectionPool::shutdown_all`] sends graceful
//!   `disconnect` to every entry on plugin teardown.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::{Mutex, RwLock};

use crate::client::{DapClient, DapClientError, SourceBreakpointSpec};
use crate::config::DapHostConfig;

/// Default backoff schedule (matches `nexus-lsp` / `nexus-mcp`).
#[must_use]
pub fn default_backoff() -> Vec<Duration> {
    vec![
        Duration::from_millis(100),
        Duration::from_millis(500),
        Duration::from_secs(2),
        Duration::from_secs(10),
        Duration::from_secs(30),
    ]
}

/// Tunables for [`ConnectionPool`].
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Inter-attempt sleeps for transient retries. Length defines the
    /// retry budget (`1 + len` total attempts).
    pub backoff: Vec<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            backoff: default_backoff(),
        }
    }
}

struct Entry {
    client: Arc<Mutex<DapClient>>,
}

/// Pool of [`DapClient`] connections keyed by adapter name.
pub struct ConnectionPool {
    cfg: PoolConfig,
    entries: RwLock<HashMap<String, Entry>>,
}

impl ConnectionPool {
    /// Create an empty pool.
    #[must_use]
    pub fn new(cfg: PoolConfig) -> Self {
        Self {
            cfg,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Look up or lazily connect a client for `adapter_name`.
    ///
    /// # Errors
    /// - [`DapClientError::Spawn`] / [`DapClientError::Handshake`] from
    ///   the underlying [`DapClient::connect`].
    /// - Returns a synthetic `Spawn` error wrapping a `NotFound` if
    ///   the adapter isn't in `cfg.adapters`.
    pub async fn get_or_connect(
        &self,
        adapter_name: &str,
        cfg: &DapHostConfig,
    ) -> Result<Arc<Mutex<DapClient>>, DapClientError> {
        if let Some(entry) = self.entries.read().await.get(adapter_name) {
            return Ok(Arc::clone(&entry.client));
        }
        let spec = cfg.adapters.get(adapter_name).ok_or_else(|| DapClientError::Spawn {
            command: adapter_name.to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("adapter '{adapter_name}' not in dap.toml"),
            ),
        })?;
        if spec.disabled {
            return Err(DapClientError::Spawn {
                command: spec.command.clone(),
                source: std::io::Error::other(format!(
                    "adapter '{adapter_name}' is disabled in dap.toml"
                )),
            });
        }
        let client = DapClient::connect(adapter_name, spec).await?;
        let arc = Arc::new(Mutex::new(client));
        let mut entries = self.entries.write().await;
        let chosen = entries
            .entry(adapter_name.to_string())
            .or_insert_with(|| Entry {
                client: Arc::clone(&arc),
            });
        Ok(Arc::clone(&chosen.client))
    }

    /// Execute `op` against the adapter's client; on a transient
    /// failure, reconnect and retry per the configured backoff. The
    /// pre-broken client's cached breakpoint set is replayed against
    /// the fresh adapter before `op` runs.
    ///
    /// # Errors
    /// - The first non-transient error.
    /// - The last transient error after the retry budget runs out.
    pub async fn call_with_reconnect<F, T>(
        &self,
        adapter_name: &str,
        cfg: &DapHostConfig,
        mut op: F,
    ) -> Result<T, DapClientError>
    where
        F: for<'a> FnMut(
            &'a Mutex<DapClient>,
        )
            -> Pin<Box<dyn std::future::Future<Output = Result<T, DapClientError>> + Send + 'a>>,
    {
        let mut last_err: Option<DapClientError> = None;
        let mut pending_resync: Option<HashMap<String, Vec<SourceBreakpointSpec>>> = None;
        let total_attempts = 1 + self.cfg.backoff.len();
        for attempt in 0..total_attempts {
            let client = self.get_or_connect(adapter_name, cfg).await?;
            if let Some(bps) = pending_resync.take() {
                replay_breakpoints(&client, &bps).await;
            }
            match op(&client).await {
                Ok(v) => return Ok(v),
                Err(e) if !e.is_transient() => return Err(e),
                Err(e) => {
                    tracing::warn!(
                        adapter = %adapter_name,
                        attempt,
                        error = %e,
                        "dap transient failure — will reconnect"
                    );
                    let bps = client.lock().await.breakpoints_snapshot().await;
                    if !bps.is_empty() {
                        pending_resync = Some(bps);
                    }
                    self.entries.write().await.remove(adapter_name);
                    last_err = Some(e);
                    if let Some(delay) = self.cfg.backoff.get(attempt) {
                        tokio::time::sleep(*delay).await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| DapClientError::NotRunning {
            adapter: adapter_name.to_string(),
        }))
    }

    /// Drop a single entry and run its graceful shutdown. Returns
    /// `true` if there was an entry to drop.
    pub async fn disconnect(&self, adapter_name: &str) -> bool {
        let entry = self.entries.write().await.remove(adapter_name);
        if let Some(entry) = entry {
            let mut client = entry.client.lock().await;
            client.shutdown().await;
            true
        } else {
            false
        }
    }

    /// Drop every entry, running graceful shutdown on each.
    pub async fn shutdown_all(&self) {
        let entries = std::mem::take(&mut *self.entries.write().await);
        for (name, entry) in entries {
            let mut client = entry.client.lock().await;
            tracing::info!(adapter = %name, "shutting down DAP adapter");
            client.shutdown().await;
        }
    }

    /// List every connected adapter. Used by `list_adapters` to
    /// surface live state alongside the configured set.
    pub async fn connected_adapters(&self) -> Vec<String> {
        self.entries.read().await.keys().cloned().collect()
    }
}

/// Re-issue the cached breakpoint set against a freshly-connected
/// adapter. Errors are logged and ignored — replay failures must not
/// burn the retry budget. The caller decides whether the *next* op
/// attempt is healthy.
async fn replay_breakpoints(
    client: &Mutex<DapClient>,
    bps: &HashMap<String, Vec<SourceBreakpointSpec>>,
) {
    let lock = client.lock().await;
    for (source, lines) in bps {
        let payload = json!({
            "source": { "path": source },
            "breakpoints": lines.iter().map(spec_to_wire).collect::<Vec<_>>(),
        });
        if let Err(err) = lock.send_request("setBreakpoints", Some(payload)).await {
            tracing::warn!(
                source = %source,
                error = %err,
                "dap resync setBreakpoints failed — continuing"
            );
        }
    }
    // The replayed set lives on the new client's cache too.
    for (source, lines) in bps {
        lock.remember_breakpoints(source, lines.clone()).await;
    }
}

fn spec_to_wire(b: &SourceBreakpointSpec) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("line".to_string(), json!(b.line));
    if let Some(c) = &b.condition {
        obj.insert("condition".to_string(), json!(c));
    }
    if let Some(h) = &b.hit_condition {
        obj.insert("hitCondition".to_string(), json!(h));
    }
    if let Some(m) = &b.log_message {
        obj.insert("logMessage".to_string(), json!(m));
    }
    serde_json::Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pool_starts_empty() {
        let pool = ConnectionPool::new(PoolConfig::default());
        assert!(pool.connected_adapters().await.is_empty());
    }

    #[tokio::test]
    async fn disconnect_unknown_returns_false() {
        let pool = ConnectionPool::new(PoolConfig::default());
        assert!(!pool.disconnect("nonexistent").await);
    }

    #[tokio::test]
    async fn get_unknown_adapter_errors_with_spawn() {
        let pool = ConnectionPool::new(PoolConfig::default());
        let cfg = DapHostConfig::default();
        let Err(err) = pool.get_or_connect("codelldb", &cfg).await else {
            panic!("expected error for unconfigured adapter");
        };
        assert!(matches!(err, DapClientError::Spawn { .. }));
    }

    #[test]
    fn spec_to_wire_omits_optional_fields() {
        let plain = SourceBreakpointSpec {
            line: 7,
            condition: None,
            hit_condition: None,
            log_message: None,
        };
        let v = spec_to_wire(&plain);
        let obj = v.as_object().unwrap();
        assert_eq!(obj["line"], json!(7));
        assert!(!obj.contains_key("condition"));
        assert!(!obj.contains_key("hitCondition"));
        assert!(!obj.contains_key("logMessage"));
    }

    #[test]
    fn spec_to_wire_includes_optional_fields_when_present() {
        let full = SourceBreakpointSpec {
            line: 10,
            condition: Some("i == 3".to_string()),
            hit_condition: Some("> 5".to_string()),
            log_message: Some("hit".to_string()),
        };
        let v = spec_to_wire(&full);
        let obj = v.as_object().unwrap();
        assert_eq!(obj["condition"], json!("i == 3"));
        assert_eq!(obj["hitCondition"], json!("> 5"));
        assert_eq!(obj["logMessage"], json!("hit"));
    }

    #[tokio::test]
    async fn disabled_adapter_errors_with_spawn() {
        use crate::config::DapAdapterSpec;
        use std::collections::HashMap;
        let mut adapters = HashMap::new();
        adapters.insert(
            "x".to_string(),
            DapAdapterSpec {
                name: "x".to_string(),
                command: "echo".to_string(),
                args: vec![],
                adapter_type: None,
                file_types: vec![],
                disabled: true,
                env: HashMap::new(),
                metadata: None,
            },
        );
        let cfg = DapHostConfig {
            adapters,
            contributed_by: HashMap::new(),
        };
        let pool = ConnectionPool::new(PoolConfig::default());
        let Err(err) = pool.get_or_connect("x", &cfg).await else {
            panic!("expected error for disabled adapter");
        };
        assert!(matches!(err, DapClientError::Spawn { .. }));
    }
}
