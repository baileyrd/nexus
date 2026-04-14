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
//!
//! # Sync vs async handlers
//!
//! Two dispatch paths coexist:
//!
//! - [`IpcDispatcher::dispatch`] — synchronous. Handlers return immediately;
//!   [`crate::PluginContext::ipc_call`] wraps the call in
//!   `tokio::task::spawn_blocking` so blocking I/O (SQLite, FS) does not
//!   starve the async runtime.
//! - [`IpcDispatcher::dispatch_async`] — asynchronous. Handlers return a
//!   `Future`; the caller awaits it directly. Preferred when a handler
//!   itself performs async work (HTTP calls, nested `ipc_call`s) because
//!   it avoids holding the plugin-loader mutex across await points.
//!
//! A handler that implements `dispatch_async` returns `Some(future)`; one
//! that is sync-only returns `None` and the caller falls back to `dispatch`.

use std::future::Future;
use std::pin::Pin;

use crate::error::IpcError;

/// A boxed, `'static`, `Send` future returned by an async IPC handler.
///
/// `'static` because the future lives past the dispatch call; handlers
/// capture any state they need by value (typically via `Arc`-clone).
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
    /// then fall back to [`dispatch`](IpcDispatcher::dispatch). The default
    /// implementation returns `None`, so existing dispatchers without async
    /// support stay correct.
    ///
    /// Implementors must not hold any shared mutex across the returned
    /// future's await points — resolve the handler synchronously, then hand
    /// control to the future.
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
