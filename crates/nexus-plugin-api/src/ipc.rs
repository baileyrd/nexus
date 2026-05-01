//! IPC dispatch abstraction — stable plugin-facing interface.

use std::future::Future;
use std::pin::Pin;

use crate::capability::Capability;
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

    /// Capabilities the caller must hold to invoke `command_id` on
    /// `target_plugin_id`, in addition to the unconditional
    /// [`Capability::IpcCall`] check the kernel context performs first.
    ///
    /// The default returns an empty list — most IPC commands need nothing
    /// beyond `IpcCall`. Override this for commands that perform high-impact
    /// side effects (process spawn, external network, …) so callers must
    /// hold the matching kernel capability rather than laundering the
    /// effect through `IpcCall` alone. See issue #77.
    ///
    /// [`KernelPluginContext::ipc_call`] consults this **before** dispatch,
    /// so handlers themselves don't need to re-check.
    fn required_caller_caps(
        &self,
        target_plugin_id: &str,
        command_id: &str,
    ) -> Vec<Capability> {
        let _ = (target_plugin_id, command_id);
        Vec::new()
    }
}
