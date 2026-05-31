//! Connection pool for ACP clients keyed by agent name.
//!
//! Mirrors [`nexus-lsp::pool`] — lazy connect, exponential-backoff
//! reconnect on transient failure, graceful shutdown on plugin
//! teardown. ACP has no document-tracking state to resync, so the
//! reconnect loop is a strict subset of LSP's: the broken entry is
//! dropped, a fresh connect runs, and the next attempt fires.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};

use crate::client::{AcpClient, AcpClientError};
use crate::config::AcpHostConfig;

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
    client: Arc<Mutex<AcpClient>>,
}

/// Pool of `AcpClient` connections keyed by agent name.
pub struct ConnectionPool {
    cfg: PoolConfig,
    /// `RwLock` so reads (get-or-connect after the first call) don't
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

    /// Look up or lazily connect a client for `agent_name`.
    ///
    /// # Errors
    /// - [`AcpClientError::Spawn`] / [`AcpClientError::Handshake`] from
    ///   the underlying [`AcpClient::connect`].
    /// - Synthetic [`AcpClientError::Spawn`] wrapping `NotFound` if
    ///   the agent isn't registered.
    pub async fn get_or_connect(
        &self,
        agent_name: &str,
        cfg: &AcpHostConfig,
    ) -> Result<Arc<Mutex<AcpClient>>, AcpClientError> {
        if let Some(entry) = self.entries.read().await.get(agent_name) {
            return Ok(Arc::clone(&entry.client));
        }
        let spec = cfg
            .adapters
            .get(agent_name)
            .ok_or_else(|| AcpClientError::Spawn {
                command: agent_name.to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("agent '{agent_name}' is not registered"),
                ),
            })?;
        if spec.disabled {
            return Err(AcpClientError::Spawn {
                command: spec.command.clone(),
                source: std::io::Error::other(format!("agent '{agent_name}' is disabled")),
            });
        }
        let client = AcpClient::connect(agent_name, spec, self.forge_root.clone()).await?;
        let arc = Arc::new(Mutex::new(client));
        let mut entries = self.entries.write().await;
        let chosen = entries
            .entry(agent_name.to_string())
            .or_insert_with(|| Entry {
                client: Arc::clone(&arc),
            });
        Ok(Arc::clone(&chosen.client))
    }

    /// Execute `op` against the agent's client; on a transient
    /// failure, reconnect and retry per the configured backoff.
    ///
    /// `op` is called once per attempt. Non-transient failures
    /// short-circuit immediately — misconfiguration / agent-rejected
    /// requests should not burn the retry budget.
    ///
    /// # Errors
    /// - The first non-transient error.
    /// - The last transient error after the retry budget runs out.
    pub async fn call_with_reconnect<F, T>(
        &self,
        agent_name: &str,
        cfg: &AcpHostConfig,
        mut op: F,
    ) -> Result<T, AcpClientError>
    where
        F: for<'a> FnMut(
            &'a Mutex<AcpClient>,
        ) -> Pin<
            Box<dyn std::future::Future<Output = Result<T, AcpClientError>> + Send + 'a>,
        >,
    {
        let mut last_err: Option<AcpClientError> = None;
        let total_attempts = 1 + self.cfg.backoff.len();
        for attempt in 0..total_attempts {
            let client = self.get_or_connect(agent_name, cfg).await?;
            match op(&client).await {
                Ok(v) => return Ok(v),
                Err(e) if !e.is_transient() => return Err(e),
                Err(e) => {
                    tracing::warn!(
                        agent = %agent_name,
                        attempt,
                        error = %e,
                        "acp transient failure — will reconnect"
                    );
                    self.entries.write().await.remove(agent_name);
                    last_err = Some(e);
                    if let Some(delay) = self.cfg.backoff.get(attempt) {
                        tokio::time::sleep(*delay).await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| AcpClientError::NotRunning {
            agent: agent_name.to_string(),
        }))
    }

    /// Drop a single entry and run its graceful shutdown. Returns
    /// `true` if there was an entry to drop.
    pub async fn disconnect(&self, agent_name: &str) -> bool {
        let entry = self.entries.write().await.remove(agent_name);
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
            tracing::info!(agent = %name, "shutting down ACP agent");
            client.shutdown().await;
        }
    }

    /// List every connected agent. Used by the `list_agents` IPC
    /// handler to report live status.
    pub async fn connected_agents(&self) -> Vec<String> {
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
        assert!(pool.connected_agents().await.is_empty());
    }

    #[tokio::test]
    async fn disconnect_unknown_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let pool = ConnectionPool::new(PoolConfig::default(), dir.path().to_path_buf());
        assert!(!pool.disconnect("ghost").await);
    }

    #[tokio::test]
    async fn get_unknown_agent_errors_with_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let pool = ConnectionPool::new(PoolConfig::default(), dir.path().to_path_buf());
        let cfg = AcpHostConfig::new();
        let Err(err) = pool.get_or_connect("hermes", &cfg).await else {
            panic!("expected error for unconfigured agent");
        };
        assert!(matches!(err, AcpClientError::Spawn { .. }));
    }
}
