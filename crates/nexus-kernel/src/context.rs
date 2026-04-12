//! The `PluginContext` trait — the full public surface a plugin sees when
//! interacting with the kernel.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::capability::Capability;
use crate::error::{IpcError, Result};
use crate::event::EventFilter;
use crate::event_bus::EventSubscription;
use crate::log::LogLevel;

/// The plugin-facing kernel API. Implemented by the kernel's
/// `KernelPluginContext` struct; plugins only see this trait object.
///
/// Capability enforcement happens inside the impl — if a plugin calls
/// `read_file` without `fs.read`, the impl short-circuits with
/// `CapabilityError::Denied` before reaching the storage backend. The
/// plugin physically cannot bypass the check because the only handle it
/// holds is `&dyn PluginContext`.
#[async_trait]
pub trait PluginContext: Send + Sync {
    // ---- Identity ----

    /// The plugin's id (reverse-DNS, e.g., "com.example.weather").
    fn plugin_id(&self) -> &str;

    /// The plugin's version string from the manifest.
    fn plugin_version(&self) -> &str;

    /// Check whether this plugin holds the given capability.
    fn has_capability(&self, cap: Capability) -> bool;

    // ---- File system (gated by fs.* capabilities) ----

    /// Read a file. Gated by `fs.read` or `fs.read.external`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks the required capability.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write a file. Gated by `fs.write` or `fs.write.external`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks the required capability.
    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()>;

    /// Delete a file. Gated by `fs.write`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `fs.write`.
    async fn delete_file(&self, path: &Path) -> Result<()>;

    /// List files in a directory (non-recursive). Gated by `fs.read`.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `fs.read`.
    async fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>>;

    // ---- KV store (gated by kv.read / kv.write) ----

    /// Get a value from the plugin's KV store. Key is plugin-local;
    /// the kernel internally namespaces it.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `kv.read`.
    async fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Set a value in the plugin's KV store.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `kv.write`.
    async fn kv_set(&self, key: &str, value: &[u8]) -> Result<()>;

    /// Delete a key from the plugin's KV store. Returns `Ok(())` even
    /// if the key doesn't exist.
    ///
    /// # Errors
    /// Returns `CapabilityError::Denied` if the plugin lacks `kv.write`.
    async fn kv_delete(&self, key: &str) -> Result<()>;

    // ---- Events ----

    /// Publish a `NexusEvent::Custom`. The `type_id` must start with the
    /// plugin's id (reverse-DNS namespace). Kernel populates metadata.
    ///
    /// # Errors
    /// - `BusError::TypeIdNamespaceMismatch` if `type_id` doesn't namespace-match.
    /// - `BusError::Closed` if the bus is shut down.
    fn publish(&self, type_id: &str, payload: serde_json::Value) -> Result<()>;

    /// Subscribe to events matching the filter. Subscription is dropped
    /// automatically when it goes out of scope.
    fn subscribe(&self, filter: EventFilter) -> EventSubscription;

    // ---- IPC (gated by ipc.call) ----

    /// Call an IPC command on another plugin. `timeout` is required.
    ///
    /// # Errors
    /// - `IpcError::PluginNotFound` if the target plugin isn't loaded.
    /// - `IpcError::CommandNotFound` if the plugin doesn't register that command.
    /// - `IpcError::Timeout` if the call takes longer than `timeout`.
    /// - `IpcError::PluginCrashedDuringCall` if the target plugin panics.
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, IpcError>;

    // ---- Logging ----

    /// Emit a log message at the given level. Plumbed through `tracing`
    /// with structured fields including `plugin_id`.
    fn log(&self, level: LogLevel, message: &str);
}
