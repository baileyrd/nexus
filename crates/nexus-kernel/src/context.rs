//! The `PluginContext` trait — the full public surface a plugin sees when
//! interacting with the kernel.
//!
//! `PluginContext` is split into six narrower supertraits — [`Identity`],
//! [`FileSystem`], [`KvAccess`], [`Events`], [`Ipc`], [`Log`] — so a
//! handler / helper that only needs a slice of the surface can depend on
//! exactly that slice (ISP). The wide [`PluginContext`] alias is kept as
//! the umbrella consumers continue to take as `&dyn PluginContext`; a
//! blanket impl auto-derives it for any type that implements all six
//! supertraits, so the only `impl PluginContext` site (in
//! [`crate::context_impl`]) is split into six per-trait impls without
//! callers needing to change.
//!
//! Capability enforcement happens inside each impl — if a plugin calls
//! `read_file` without `fs.read`, the impl short-circuits with
//! `CapabilityError::Denied` before reaching the storage backend. The
//! plugin physically cannot bypass the check because the only handle it
//! holds is `&dyn PluginContext` (or one of the narrower supertrait
//! objects).

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::capability::Capability;
use crate::error::{IpcError, Result};
use crate::event::EventFilter;
use crate::event_bus::EventSubscription;
use crate::log::LogLevel;

// ---- Identity --------------------------------------------------------

/// Plugin identity + capability introspection. Held by every helper that
/// only needs to know *who* it is running as.
pub trait Identity: Send + Sync {
    /// The plugin's id (reverse-DNS, e.g., "com.example.weather").
    fn plugin_id(&self) -> &str;

    /// The plugin's version string from the manifest.
    fn plugin_version(&self) -> &str;

    /// Check whether this plugin holds the given capability.
    fn has_capability(&self, cap: Capability) -> bool;
}

// ---- File system -----------------------------------------------------

/// File-system access, gated by `fs.read` / `fs.write` capabilities.
#[async_trait]
pub trait FileSystem: Send + Sync {
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
}

// ---- KV store --------------------------------------------------------

/// Plugin-local key/value store, gated by `kv.read` / `kv.write`.
#[async_trait]
pub trait KvAccess: Send + Sync {
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
}

// ---- Events ----------------------------------------------------------

/// Event-bus publish/subscribe surface.
pub trait Events: Send + Sync {
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
}

// ---- IPC -------------------------------------------------------------

/// Plugin-to-plugin IPC, gated by `ipc.call`.
#[async_trait]
pub trait Ipc: Send + Sync {
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
}

// ---- Logging ---------------------------------------------------------

/// Structured logging at a chosen level. Pure-compute plugins typically
/// depend only on [`Identity`] + [`Log`].
pub trait Log: Send + Sync {
    /// Emit a log message at the given level. Plumbed through `tracing`
    /// with structured fields including `plugin_id`.
    fn log(&self, level: LogLevel, message: &str);
}

// ---- Umbrella --------------------------------------------------------

/// The wide plugin-facing kernel API. This trait is now a marker that
/// composes the six narrower supertraits; the concrete kernel impl
/// ([`crate::KernelPluginContext`]) implements each supertrait
/// individually, and the blanket impl below auto-derives
/// `PluginContext` for it.
///
/// Handlers and helpers should depend on the *narrowest* supertrait
/// they actually use (e.g. a pure-compute helper that just needs to log
/// can take `&(impl Identity + Log)` or `&dyn Log`). Callers that need
/// the whole surface continue to use `&dyn PluginContext` unchanged.
pub trait PluginContext: Identity + FileSystem + KvAccess + Events + Ipc + Log {}

impl<T> PluginContext for T where T: Identity + FileSystem + KvAccess + Events + Ipc + Log + ?Sized
{}
