//! Connection pool for LSP clients keyed by server name.
//!
//! Mirrors [`nexus-mcp::ConnectionPool`](../nexus_mcp/struct.ConnectionPool.html)
//! at a smaller scale:
//!
//! - **Lazy connect** — first call to [`ConnectionPool::get_or_connect`]
//!   spawns the child process; subsequent calls reuse the live entry.
//! - **Reconnect with backoff** — [`ConnectionPool::call_with_reconnect`]
//!   wraps an op closure; transient failures (broken pipe, request
//!   timeout, `NotRunning`) trigger a reconnect and retry against the
//!   configured backoff schedule.
//! - **Shutdown all** — [`ConnectionPool::shutdown_all`] sends graceful
//!   `shutdown`/`exit` to every entry on plugin teardown.
//!
//! There's no idle eviction yet — LSP servers are workspace-scoped and
//! the user opens one forge at a time, so the entry count stays small.
//! Add eviction if a future use case (e.g. project-per-folder) needs it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};

use crate::client::{LspClient, LspClientError, OpenDocument};
use crate::config::LspHostConfig;

/// Default backoff schedule (matches `nexus-mcp`).
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
    client: Arc<Mutex<LspClient>>,
}

/// Pool of `LspClient` connections keyed by server name.
pub struct ConnectionPool {
    cfg: PoolConfig,
    /// `RwLock` so reads (`get_or_connect` after the first call) don't
    /// serialise on the hot path.
    entries: RwLock<HashMap<String, Entry>>,
    /// Forge root passed to each connect.
    forge_root: PathBuf,
}

impl ConnectionPool {
    /// Create an empty pool.
    #[must_use]
    pub fn new(cfg: PoolConfig, forge_root: PathBuf) -> Self {
        Self {
            cfg,
            entries: RwLock::new(HashMap::new()),
            forge_root,
        }
    }

    /// Look up or lazily connect a client for `server_name`.
    ///
    /// # Errors
    /// - [`LspClientError::Spawn`] / [`LspClientError::Handshake`] from the
    ///   underlying [`LspClient::connect`].
    /// - Returns a synthetic `Spawn` error wrapping a `NotFound` if the
    ///   server isn't in `cfg.servers`.
    pub async fn get_or_connect(
        &self,
        server_name: &str,
        cfg: &LspHostConfig,
    ) -> Result<Arc<Mutex<LspClient>>, LspClientError> {
        if let Some(entry) = self.entries.read().await.get(server_name) {
            return Ok(Arc::clone(&entry.client));
        }
        // Cache miss — connect.
        let spec = cfg
            .servers
            .get(server_name)
            .ok_or_else(|| LspClientError::Spawn {
                command: server_name.to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("server '{server_name}' not in lsp.toml"),
                ),
            })?;
        if spec.disabled {
            return Err(LspClientError::Spawn {
                command: spec.command.clone(),
                source: std::io::Error::other(format!(
                    "server '{server_name}' is disabled in lsp.toml"
                )),
            });
        }
        let client = LspClient::connect(server_name, spec, self.forge_root.clone()).await?;
        let arc = Arc::new(Mutex::new(client));
        let mut entries = self.entries.write().await;
        // Race: another caller may have inserted while we connected.
        // Prefer the existing entry to avoid leaking a child process —
        // ours will drop and be reaped by `kill_on_drop`.
        let chosen = entries
            .entry(server_name.to_string())
            .or_insert_with(|| Entry {
                client: Arc::clone(&arc),
            });
        Ok(Arc::clone(&chosen.client))
    }

    /// Execute `op` against the server's client; on a transient
    /// failure, reconnect and retry per the configured backoff.
    ///
    /// `op` is called with the locked client; non-transient failures
    /// short-circuit immediately so misconfiguration (handshake error,
    /// JSON-RPC method-not-found) doesn't burn the retry budget.
    ///
    /// # Errors
    /// - The first non-transient error.
    /// - The last transient error after the retry budget runs out.
    pub async fn call_with_reconnect<F, T>(
        &self,
        server_name: &str,
        cfg: &LspHostConfig,
        mut op: F,
    ) -> Result<T, LspClientError>
    where
        F: for<'a> FnMut(
            &'a Mutex<LspClient>,
        ) -> Pin<
            Box<dyn std::future::Future<Output = Result<T, LspClientError>> + Send + 'a>,
        >,
    {
        let mut last_err: Option<LspClientError> = None;
        // Documents to replay against the next freshly-connected
        // client. Populated from the broken client's snapshot
        // before we drop the entry; consumed after the next
        // `get_or_connect` returns. Stays `None` on the first
        // attempt and on any successful retry chain.
        let mut pending_resync: Option<Vec<OpenDocument>> = None;
        let total_attempts = 1 + self.cfg.backoff.len();
        for attempt in 0..total_attempts {
            let client = self.get_or_connect(server_name, cfg).await?;
            // Replay any open documents we captured from the
            // previously-broken client. Replay errors are logged
            // and ignored — the next `op` call will surface a
            // fresh transient error if the new server is also
            // unhealthy, and we'd rather not burn the retry budget
            // here on resync hiccups.
            if let Some(docs) = pending_resync.take() {
                let lock = client.lock().await;
                for doc in &docs {
                    if let Err(err) = lock
                        .did_open(&doc.uri, &doc.language_id, doc.version, &doc.text)
                        .await
                    {
                        tracing::warn!(
                            server = %server_name,
                            uri = %doc.uri,
                            error = %err,
                            "lsp resync did_open failed — continuing"
                        );
                    }
                }
            }
            match op(&client).await {
                Ok(v) => return Ok(v),
                Err(e) if !e.is_transient() => return Err(e),
                Err(e) => {
                    tracing::warn!(
                        server = %server_name,
                        attempt,
                        error = %e,
                        "lsp transient failure — will reconnect"
                    );
                    // Snapshot the broken client's document set
                    // *before* removing the entry — once dropped
                    // the LspClient is gone, taking its `documents`
                    // map with it. The snapshot is what the next
                    // attempt replays.
                    let docs = client.lock().await.documents_snapshot().await;
                    if !docs.is_empty() {
                        pending_resync = Some(docs);
                    }
                    // Drop the broken entry so the next
                    // get_or_connect spawns a fresh child.
                    self.entries.write().await.remove(server_name);
                    last_err = Some(e);
                    if let Some(delay) = self.cfg.backoff.get(attempt) {
                        tokio::time::sleep(*delay).await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| LspClientError::NotRunning {
            server: server_name.to_string(),
        }))
    }

    /// Drop a single entry and run its graceful shutdown. Returns
    /// `true` if there was an entry to drop.
    pub async fn disconnect(&self, server_name: &str) -> bool {
        let entry = self.entries.write().await.remove(server_name);
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
            tracing::info!(server = %name, "shutting down LSP server");
            client.shutdown().await;
        }
    }

    /// List every connected server. Used by the `list_servers` IPC
    /// handler to report live status.
    pub async fn connected_servers(&self) -> Vec<String> {
        self.entries.read().await.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pool_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let pool = ConnectionPool::new(PoolConfig::default(), dir.path().to_path_buf());
        assert!(pool.connected_servers().await.is_empty());
    }

    #[tokio::test]
    async fn disconnect_unknown_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let pool = ConnectionPool::new(PoolConfig::default(), dir.path().to_path_buf());
        assert!(!pool.disconnect("nonexistent").await);
    }

    #[tokio::test]
    async fn get_unknown_server_errors_with_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let pool = ConnectionPool::new(PoolConfig::default(), dir.path().to_path_buf());
        let cfg = LspHostConfig::default();
        // unwrap_err requires the Ok variant to be Debug — `LspClient`
        // intentionally isn't, so destructure manually.
        let Err(err) = pool.get_or_connect("rust-analyzer", &cfg).await else {
            panic!("expected error for unconfigured server");
        };
        assert!(matches!(err, LspClientError::Spawn { .. }));
    }
}
