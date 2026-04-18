//! IPC dispatch abstraction — stable plugin-facing interface.

use std::future::Future;
use std::pin::Pin;

use crate::error::IpcError;

/// A boxed, `'static`, `Send` future returned by an async IPC handler.
pub type IpcFuture = Pin<Box<dyn Future<Output = Result<serde_json::Value, IpcError>> + Send>>;

/// Dispatches an IPC command to a loaded plugin's handler.
///
/// The caller's capability check is performed by the kernel context before
/// delegating here; implementations only resolve the target and invoke the
/// handler.
pub trait IpcDispatcher: Send + Sync {
    /// Dispatch `command_id` on plugin `target_plugin_id` with `args`.
    ///
    /// # Errors
    /// - [`IpcError::PluginNotFound`] if the target plugin is not loaded.
    /// - [`IpcError::CommandNotFound`] if the target does not register that command.
    /// - [`IpcError::PluginCrashedDuringCall`] on panic or execution error.
    fn dispatch(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, IpcError>;

    /// Try to dispatch `command_id` asynchronously.
    ///
    /// Returns `Some(future)` when the target plugin has an async handler for
    /// this command; returns `None` when it is sync-only — the caller should
    /// then fall back to [`dispatch`](IpcDispatcher::dispatch).
    fn dispatch_async(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
    ) -> Option<IpcFuture> {
        let _ = (target_plugin_id, command_id, args);
        None
    }
}
