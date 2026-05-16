//! BL-140 Phase 2b — `IpcInvoker` trait that abstracts over local and
//! remote IPC dispatch.
//!
//! Local invocations route through [`KernelPluginContext::ipc_call`];
//! remote invocations route through a JSON-RPC client. The CLI uses
//! the trait so the same subcommand body works against both shapes
//! without knowing which transport is in play.
//!
//! # Scope
//!
//! The trait covers only `ipc_call` — every other [`PluginContext`]
//! method (kv, log, publish, subscribe, …) is local-only and gated on
//! the concrete `KernelPluginContext`. None of the existing CLI
//! subcommands use those methods today; if a future subcommand needs
//! one, the right move is to expose it through a dedicated IPC verb
//! on a service plugin rather than widening this trait.
//!
//! # Errors
//!
//! Local `IpcError` and remote JSON-RPC `error` envelopes are
//! unified under [`IpcInvokerError`]. Callers that need to inspect
//! local kernel-side details can pattern-match on `Local`; remote
//! errors carry the JSON-RPC `code` + `message` verbatim.

use std::time::Duration;

use nexus_kernel::{IpcError, KernelPluginContext, PluginContext};
use serde_json::Value;

/// Errors raised by an [`IpcInvoker::ipc_call`] dispatch.
///
/// The local path produces [`IpcInvokerError::Local`] wrapping a kernel
/// [`IpcError`]; the remote path produces one of `Remote`, `Transport`,
/// or `Timeout`.
#[derive(Debug, thiserror::Error)]
pub enum IpcInvokerError {
    /// Local kernel-side IPC error.
    #[error("{0}")]
    Local(#[from] IpcError),
    /// The remote forge server replied with a JSON-RPC error envelope.
    #[error("remote server error (code {code}): {message}")]
    Remote {
        /// JSON-RPC error code (e.g. -32000, -32601, -32602).
        code: i64,
        /// Server-supplied error message verbatim.
        message: String,
    },
    /// Underlying transport failed — connection dropped, malformed
    /// frame, or the response router shut down.
    #[error("transport error: {0}")]
    Transport(String),
    /// The per-call deadline elapsed before a response arrived.
    #[error("IPC call to '{plugin_id}'.'{command}' timed out after {timeout_ms}ms")]
    Timeout {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
        /// Timeout that was exceeded.
        timeout_ms: u64,
    },
}

/// Trait surface shared by local and remote IPC dispatch.
///
/// Implementors:
/// - [`LocalIpcInvoker`] wraps a [`KernelPluginContext`].
/// - `nexus_remote::RemoteIpcInvoker` wraps an `Arc<RemoteClient>`.
#[async_trait::async_trait]
pub trait IpcInvoker: Send + Sync {
    /// Dispatch an IPC call against the target plugin / command.
    ///
    /// # Errors
    /// See [`IpcInvokerError`]. Local errors round-trip through
    /// [`From<IpcError>`]; remote errors come back as `Remote` /
    /// `Transport` / `Timeout`.
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, IpcInvokerError>;
}

/// `IpcInvoker` implementation backed by a live local kernel
/// [`KernelPluginContext`].
///
/// Held behind an `Arc` so the trait object can be cheaply cloned
/// without copying the underlying context.
pub struct LocalIpcInvoker {
    context: KernelPluginContext,
}

impl LocalIpcInvoker {
    /// Wrap an existing kernel context.
    #[must_use]
    pub fn new(context: KernelPluginContext) -> Self {
        Self { context }
    }

    /// Borrow the wrapped context. Useful for callers that need
    /// access to non-IPC methods (kv, publish, subscribe).
    #[must_use]
    pub fn context(&self) -> &KernelPluginContext {
        &self.context
    }
}

#[async_trait::async_trait]
impl IpcInvoker for LocalIpcInvoker {
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, IpcInvokerError> {
        self.context
            .ipc_call(target_plugin_id, command_id, args, timeout)
            .await
            .map_err(IpcInvokerError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_error_converts_into_local_variant() {
        let err = IpcError::PluginNotFound {
            plugin_id: "com.x".to_string(),
        };
        let wrapped: IpcInvokerError = err.into();
        match &wrapped {
            IpcInvokerError::Local(IpcError::PluginNotFound { plugin_id }) => {
                assert_eq!(plugin_id, "com.x");
            }
            other => panic!("expected Local(PluginNotFound), got: {other}"),
        }
    }

    #[test]
    fn timeout_displays_with_canonical_format() {
        let err = IpcInvokerError::Timeout {
            plugin_id: "com.x".to_string(),
            command: "y".to_string(),
            timeout_ms: 1500,
        };
        let s = err.to_string();
        assert!(s.contains("com.x"));
        assert!(s.contains('y'));
        assert!(s.contains("1500"));
    }

    #[test]
    fn remote_displays_code_and_message() {
        let err = IpcInvokerError::Remote {
            code: -32000,
            message: "boom".to_string(),
        };
        let s = err.to_string();
        assert!(s.contains("-32000"));
        assert!(s.contains("boom"));
    }
}
