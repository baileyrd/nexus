//! IPC dispatch abstraction.
//!
//! The kernel defines the [`IpcDispatcher`] trait; the plugins crate (which
//! already depends on the kernel) implements it on top of its `PluginLoader`.
//! This inverts the dependency so the kernel can route IPC calls to plugins
//! without importing the plugin runtime — keeping kernel containment intact.
//!
//! Used by [`crate::PluginContext::ipc_call`] via the optional dispatcher
//! handle held on [`crate::KernelPluginContext`]. When the handle is absent
//! (e.g. unit tests or kernels booted without a loader), `ipc_call` reports
//! [`IpcError::PluginNotFound`].

use crate::error::IpcError;

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
}
