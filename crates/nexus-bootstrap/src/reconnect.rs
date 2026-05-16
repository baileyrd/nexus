//! BL-140 Phase 2c — reconnect-on-drop wrapper over [`RemoteRuntime`].
//!
//! When the underlying transport dies (SSH child exits, network drops,
//! response router stops) the bare [`crate::remote::RemoteRuntime`]
//! reports every subsequent call as
//! [`IpcInvokerError::Transport`] forever. This module layers
//! reconnect-with-backoff on top: when a call fails with `Transport`,
//! the wrapper tears down the current runtime, asks a
//! [`ConnectionFactory`] for a fresh one, and retries the call.
//!
//! # Reconnect triggers
//!
//! Only `IpcInvokerError::Transport(_)` triggers a reconnect. Server-
//! reported errors (`Remote { code, message }`) and per-call
//! deadlines (`Timeout`) don't — they're not connection-death
//! signals.
//!
//! # Backoff
//!
//! Default schedule: `[100ms, 500ms, 2s, 10s, 30s]`. After the last
//! delay is consumed the next failure surfaces as a final
//! `Transport(_)` to the caller. The schedule is configurable via
//! [`ReconnectingRuntime::with_backoff`].
//!
//! # Subscription replay
//!
//! Today the reconnect path drops every active subscription on the
//! floor — the new connection is empty server-side until the caller
//! re-issues `subscribe`. This is fine because no CLI subcommand
//! currently subscribes through the remote path; subscriptions are
//! reserved for the local kernel event bus. When a remote subscriber
//! lands (Phase 3 shell, or a future CLI verb that watches events
//! over SSH) the wrapper will need to replay every active
//! subscription server-side on reconnect. Filed as a follow-up.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::invoker::{IpcInvoker, IpcInvokerError};
use crate::remote::{build_remote_runtime_ssh, RemoteRuntime};
use nexus_remote::ForgeUri;

/// Default reconnect schedule — five attempts spaced
/// `[100ms, 500ms, 2s, 10s, 30s]`. Matches the `nexus-mcp`
/// `ConnectionPool` pattern.
pub const DEFAULT_BACKOFF: [Duration; 5] = [
    Duration::from_millis(100),
    Duration::from_millis(500),
    Duration::from_secs(2),
    Duration::from_secs(10),
    Duration::from_secs(30),
];

/// Factory for building a fresh [`RemoteRuntime`] when the current
/// connection dies.
///
/// Returns a `Pin<Box<dyn Future>>` rather than `async fn` because
/// trait objects with async methods need an explicit Box+Pin wrapping
/// (object-safety) — same shape `nexus-mcp` uses for its connection
/// factory.
pub trait ConnectionFactory: Send + Sync {
    /// Build a fresh runtime. Called on the very first `invoker()`
    /// dispatch and on every reconnect attempt.
    fn build<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>>;
}

/// SSH-backed factory — spawns `ssh ... nexus serve --stdio` on each
/// build. The URI is captured by value so the factory can be cloned
/// into long-lived async tasks.
pub struct SshConnectionFactory {
    uri: ForgeUri,
}

impl SshConnectionFactory {
    /// Build a factory for the supplied `ssh://` URI.
    #[must_use]
    pub fn new(uri: ForgeUri) -> Self {
        Self { uri }
    }
}

impl ConnectionFactory for SshConnectionFactory {
    fn build<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>> {
        Box::pin(async move { build_remote_runtime_ssh(&self.uri) })
    }
}

/// A self-reconnecting wrapper over a [`RemoteRuntime`]. Holds the
/// current connection behind a `Mutex` and lazily builds + rebuilds it
/// via a [`ConnectionFactory`].
pub struct ReconnectingRuntime {
    factory: Arc<dyn ConnectionFactory>,
    current: Arc<Mutex<Option<RemoteRuntime>>>,
    backoff: Vec<Duration>,
}

impl ReconnectingRuntime {
    /// Construct a reconnecting runtime backed by `factory`. The first
    /// call to [`Self::invoker`] + dispatch triggers the first
    /// `factory.build()`.
    #[must_use]
    pub fn new(factory: Arc<dyn ConnectionFactory>) -> Self {
        Self {
            factory,
            current: Arc::new(Mutex::new(None)),
            backoff: DEFAULT_BACKOFF.to_vec(),
        }
    }

    /// Override the reconnect schedule (mostly useful in tests — a
    /// tight schedule keeps test runtime down without changing
    /// semantics).
    #[must_use]
    pub fn with_backoff(mut self, schedule: Vec<Duration>) -> Self {
        self.backoff = schedule;
        self
    }

    /// Return an [`IpcInvoker`] trait object. Cheap — every dispatch
    /// re-acquires the current-runtime mutex internally so the same
    /// invoker survives reconnects.
    #[must_use]
    pub fn invoker(&self) -> Arc<dyn IpcInvoker + Send + Sync> {
        Arc::new(ReconnectingInvoker {
            factory: Arc::clone(&self.factory),
            current: Arc::clone(&self.current),
            backoff: self.backoff.clone(),
        })
    }

    /// Tear down the current connection (if any). Subsequent dispatches
    /// will rebuild on demand.
    pub async fn reset(&self) {
        let mut slot = self.current.lock().await;
        if let Some(runtime) = slot.take() {
            runtime.shutdown().await;
        }
    }

    /// Ensure a connection exists, building one through the factory
    /// if not, and return the underlying [`nexus_remote::RemoteClient`].
    ///
    /// BL-140 Phase 3 — needed by callers that want to drive
    /// [`nexus_remote::RemoteClient::subscribe`] directly (the
    /// `IpcInvoker` trait only covers `ipc_call`). The Tauri bridge
    /// uses this to wire `kernel_subscribe` over remote.
    ///
    /// The returned `Arc` clones the underlying client; the caller's
    /// reference outlives subsequent reconnects, but subscriptions
    /// installed against it are NOT replayed — when the current
    /// connection is reset, the old client's router stops and
    /// subscriptions silently drop. The frontend is expected to
    /// re-subscribe after reconnect; we'll add automatic replay when
    /// a use case actually demands it.
    ///
    /// # Errors
    /// Same shape as the first attempt of `ipc_call` — a build failure
    /// surfaces as `Transport("initial connection: ...")`.
    pub async fn ensure_client(
        &self,
    ) -> Result<Arc<nexus_remote::RemoteClient>, IpcInvokerError> {
        let mut slot = self.current.lock().await;
        if slot.is_none() {
            match self.factory.build().await {
                Ok(rt) => *slot = Some(rt),
                Err(e) => {
                    return Err(IpcInvokerError::Transport(format!(
                        "initial connection: {e}"
                    )));
                }
            }
        }
        Ok(Arc::clone(&slot.as_ref().expect("just set").client))
    }
}

struct ReconnectingInvoker {
    factory: Arc<dyn ConnectionFactory>,
    current: Arc<Mutex<Option<RemoteRuntime>>>,
    backoff: Vec<Duration>,
}

impl ReconnectingInvoker {
    /// Ensure a connection exists. If `current` is `None`, build one
    /// through the factory. Errors during build surface as
    /// `Transport` — the caller is doing an IPC call and a connection
    /// failure is the closest semantic.
    async fn ensure_connected(&self) -> Result<(), IpcInvokerError> {
        let mut slot = self.current.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        match self.factory.build().await {
            Ok(rt) => {
                *slot = Some(rt);
                Ok(())
            }
            Err(e) => Err(IpcInvokerError::Transport(format!(
                "initial connection: {e}"
            ))),
        }
    }

    /// Capture an `Arc<dyn IpcInvoker>` for the current runtime so the
    /// caller can dispatch a single call without holding the mutex
    /// across an `await` on the underlying ipc layer.
    async fn snapshot_invoker(
        &self,
    ) -> Option<Arc<dyn IpcInvoker + Send + Sync>> {
        let slot = self.current.lock().await;
        slot.as_ref().map(RemoteRuntime::invoker)
    }

    /// Tear down whatever connection sits in `current` (if any). The
    /// next `ensure_connected` will rebuild.
    async fn tear_down_current(&self) {
        let taken = {
            let mut slot = self.current.lock().await;
            slot.take()
        };
        if let Some(runtime) = taken {
            runtime.shutdown().await;
        }
    }
}

#[async_trait::async_trait]
impl IpcInvoker for ReconnectingInvoker {
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, IpcInvokerError> {
        self.ensure_connected().await?;
        let first_attempt = match self.snapshot_invoker().await {
            Some(inv) => {
                inv.ipc_call(target_plugin_id, command_id, args.clone(), timeout)
                    .await
            }
            None => {
                return Err(IpcInvokerError::Transport(
                    "ensure_connected reported ok but invoker is missing".to_string(),
                ));
            }
        };
        // Only Transport failures trigger reconnect. Server errors +
        // timeouts surface as-is.
        let Err(IpcInvokerError::Transport(first_err)) = first_attempt else {
            return first_attempt;
        };
        tracing::warn!(
            error = %first_err,
            plugin_id = %target_plugin_id,
            command_id = %command_id,
            "remote ipc_call transport failure; will attempt reconnect"
        );
        self.tear_down_current().await;

        // Walk the backoff schedule once. Each tick: sleep, rebuild,
        // retry. First successful retry wins. After the schedule
        // exhausts surface the last error.
        let mut last_err = first_err;
        for (idx, delay) in self.backoff.iter().enumerate() {
            tokio::time::sleep(*delay).await;
            let build_result = self.factory.build().await;
            let rt = match build_result {
                Ok(rt) => rt,
                Err(e) => {
                    last_err = format!("reconnect attempt {}: {e}", idx + 1);
                    tracing::warn!(
                        attempt = idx + 1,
                        error = %last_err,
                        "reconnect build failed"
                    );
                    continue;
                }
            };
            // Take the lock, install the new runtime, snapshot its
            // invoker, drop the lock before awaiting the retry call.
            let inv = {
                let mut slot = self.current.lock().await;
                *slot = Some(rt);
                slot.as_ref().expect("just set").invoker()
            };
            match inv
                .ipc_call(target_plugin_id, command_id, args.clone(), timeout)
                .await
            {
                Ok(v) => {
                    tracing::info!(
                        attempt = idx + 1,
                        plugin_id = %target_plugin_id,
                        command_id = %command_id,
                        "remote ipc_call recovered after reconnect"
                    );
                    return Ok(v);
                }
                Err(IpcInvokerError::Transport(e)) => {
                    last_err = format!("reconnect attempt {}: {e}", idx + 1);
                    self.tear_down_current().await;
                }
                Err(other) => return Err(other),
            }
        }
        Err(IpcInvokerError::Transport(format!(
            "reconnect schedule exhausted: {last_err}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_remote::SshForgeUri;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A factory that always fails. Used to verify the backoff
    /// schedule + the "schedule exhausted" surfacing.
    struct AlwaysFailFactory {
        attempts: Arc<AtomicUsize>,
    }

    impl ConnectionFactory for AlwaysFailFactory {
        fn build<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>> {
            let attempts = Arc::clone(&self.attempts);
            Box::pin(async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::anyhow!("simulated build failure"))
            })
        }
    }

    #[tokio::test]
    async fn initial_connection_failure_surfaces_as_transport_error() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let factory = Arc::new(AlwaysFailFactory {
            attempts: Arc::clone(&attempts),
        });
        let rt = ReconnectingRuntime::new(factory);
        let inv = rt.invoker();
        let err = inv
            .ipc_call("com.x", "y", Value::Null, Duration::from_secs(1))
            .await
            .unwrap_err();
        match err {
            IpcInvokerError::Transport(msg) => {
                assert!(msg.contains("initial connection"), "got: {msg}");
            }
            other => panic!("expected Transport, got: {other}"),
        }
        // One build attempt — the initial connect. The reconnect path
        // never engages because we never got a first connection.
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn default_backoff_is_the_documented_schedule() {
        assert_eq!(
            DEFAULT_BACKOFF,
            [
                Duration::from_millis(100),
                Duration::from_millis(500),
                Duration::from_secs(2),
                Duration::from_secs(10),
                Duration::from_secs(30),
            ]
        );
    }

    #[test]
    fn ssh_connection_factory_is_constructible() {
        // Smoke test — the factory just captures the URI. Real spawn
        // behaviour is covered by the integration tests over duplex
        // pairs.
        let uri = ForgeUri::Ssh(SshForgeUri {
            user: Some("alice".to_string()),
            host: "host".to_string(),
            port: Some(22),
            path: "/srv/forge".to_string(),
        });
        let _factory = SshConnectionFactory::new(uri);
    }
}
