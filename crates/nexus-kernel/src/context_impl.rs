//! `KernelPluginContext` — the concrete implementation of [`PluginContext`]
//! that the kernel provides to each plugin.
//!
//! Every capability-gated method checks the plugin's [`CapabilitySet`] before
//! performing any work and returns [`Error::Capability`] on denial. Path
//! operations are confined to `forge_root` via a canonicalize-then-prefix
//! check to prevent traversal attacks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::capability::{Capability, CapabilitySet};
use crate::context::PluginContext;
use crate::error::{BusError, CapabilityError, Error, IpcError, Result};
use crate::event::EventFilter;
use crate::event_bus::{EventBus, EventSubscription};
use crate::ipc::IpcDispatcher;
use crate::kv_store::KvStore;
use crate::log::LogLevel;

/// Concrete kernel implementation of [`PluginContext`].
///
/// Constructed by the plugin loader when a plugin is instantiated. Holds
/// shared handles to kernel services; cheap to clone as Arc-backed.
pub struct KernelPluginContext {
    plugin_id: String,
    plugin_version: String,
    capabilities: CapabilitySet,
    kv: Arc<dyn KvStore>,
    event_bus: Arc<EventBus>,
    /// Canonical form of the forge root, used for path confinement.
    forge_root_canonical: PathBuf,
    /// Optional dispatcher for plugin-to-plugin IPC. `None` means this context
    /// was built without a plugin loader (e.g. in unit tests) and `ipc_call`
    /// will return [`IpcError::DispatcherUnavailable`].
    ipc_dispatcher: Option<Arc<dyn IpcDispatcher>>,
}

impl KernelPluginContext {
    /// Create a new context for the given plugin.
    ///
    /// `forge_root` is canonicalized once at construction time so all
    /// subsequent path checks are fast prefix comparisons.
    ///
    /// # Errors
    /// Returns `Error::Io` if `forge_root` cannot be canonicalized (e.g. it
    /// doesn't exist yet).
    pub fn new(
        plugin_id: impl Into<String>,
        plugin_version: impl Into<String>,
        capabilities: CapabilitySet,
        kv: Arc<dyn KvStore>,
        event_bus: Arc<EventBus>,
        forge_root: &Path,
        ipc_dispatcher: Option<Arc<dyn IpcDispatcher>>,
    ) -> Result<Self> {
        let forge_root_canonical = forge_root.canonicalize()?;
        Ok(Self {
            plugin_id: plugin_id.into(),
            plugin_version: plugin_version.into(),
            capabilities,
            kv,
            event_bus,
            forge_root_canonical,
            ipc_dispatcher,
        })
    }

    /// Check that the plugin holds `cap`, logging a denial and returning an
    /// error if not.
    fn require_capability(&self, cap: Capability) -> Result<()> {
        if self.capabilities.contains(cap) {
            return Ok(());
        }
        tracing::warn!(
            audit = true,
            plugin_id = %self.plugin_id,
            capability = %cap,
            result = "denied",
            "capability denied"
        );
        Err(CapabilityError::Denied {
            plugin_id: self.plugin_id.clone(),
            cap,
        }
        .into())
    }

    /// Canonicalize `path` and verify it falls within `forge_root`.
    ///
    /// Returns the canonical path on success, or an `Error::Io` wrapping
    /// a permission-denied error if the path escapes the forge root.
    fn confine_path(&self, path: &Path) -> Result<PathBuf> {
        // Resolve relative to forge_root if the path is relative.
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.forge_root_canonical.join(path)
        };

        let canonical = absolute.canonicalize().map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("path '{}': {e}", absolute.display()),
            ))
        })?;

        if !canonical.starts_with(&self.forge_root_canonical) {
            tracing::warn!(
                audit = true,
                plugin_id = %self.plugin_id,
                requested_path = %canonical.display(),
                forge_root = %self.forge_root_canonical.display(),
                "path traversal denied"
            );
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "path traversal denied: '{}' is outside forge root '{}'",
                    canonical.display(),
                    self.forge_root_canonical.display()
                ),
            )));
        }

        Ok(canonical)
    }
}

#[async_trait]
impl PluginContext for KernelPluginContext {
    // ---- Identity --------------------------------------------------------

    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn plugin_version(&self) -> &str {
        &self.plugin_version
    }

    fn has_capability(&self, cap: Capability) -> bool {
        self.capabilities.contains(cap)
    }

    // ---- File system -----------------------------------------------------

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        // External paths require the stronger capability.
        let needed = if path.is_absolute()
            && !path.starts_with(&self.forge_root_canonical)
        {
            Capability::FsReadExternal
        } else {
            Capability::FsRead
        };
        self.require_capability(needed)?;
        let confined = self.confine_path(path)?;
        tokio::fs::read(&confined).await.map_err(Error::Io)
    }

    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let needed = if path.is_absolute()
            && !path.starts_with(&self.forge_root_canonical)
        {
            Capability::FsWriteExternal
        } else {
            Capability::FsWrite
        };
        self.require_capability(needed)?;
        // For writes the path may not exist yet — confine against parent dir.
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.forge_root_canonical.join(path)
        };
        // Ensure the target is inside forge_root by checking the parent.
        if let Some(parent) = absolute.parent() {
            if parent.exists() {
                let canon_parent = parent.canonicalize()?;
                if !canon_parent.starts_with(&self.forge_root_canonical) {
                    tracing::warn!(
                        audit = true,
                        plugin_id = %self.plugin_id,
                        requested_path = %absolute.display(),
                        forge_root = %self.forge_root_canonical.display(),
                        "path traversal denied"
                    );
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "path traversal denied: '{}' is outside forge root",
                            absolute.display()
                        ),
                    )));
                }
            }
        }
        tokio::fs::write(&absolute, contents).await.map_err(Error::Io)
    }

    async fn delete_file(&self, path: &Path) -> Result<()> {
        self.require_capability(Capability::FsWrite)?;
        let confined = self.confine_path(path)?;
        tokio::fs::remove_file(&confined).await.map_err(Error::Io)
    }

    async fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        self.require_capability(Capability::FsRead)?;
        let confined = self.confine_path(dir)?;
        let mut entries = tokio::fs::read_dir(&confined).await.map_err(Error::Io)?;
        let mut paths = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(Error::Io)? {
            paths.push(entry.path());
        }
        Ok(paths)
    }

    // ---- KV store --------------------------------------------------------

    async fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.require_capability(Capability::KvRead)?;
        self.kv
            .get(&self.plugin_id, key)
            .map_err(Error::Kv)
    }

    async fn kv_set(&self, key: &str, value: &[u8]) -> Result<()> {
        self.require_capability(Capability::KvWrite)?;
        self.kv
            .set(&self.plugin_id, key, value)
            .map_err(Error::Kv)
    }

    async fn kv_delete(&self, key: &str) -> Result<()> {
        self.require_capability(Capability::KvWrite)?;
        self.kv
            .delete(&self.plugin_id, key)
            .map_err(Error::Kv)
    }

    // ---- Events ----------------------------------------------------------

    fn publish(&self, type_id: &str, payload: serde_json::Value) -> Result<()> {
        if !type_id.starts_with(&self.plugin_id as &str) {
            return Err(BusError::TypeIdNamespaceMismatch {
                plugin_id: self.plugin_id.clone(),
                type_id: type_id.to_string(),
            }
            .into());
        }
        self.event_bus
            .publish_plugin(&self.plugin_id, type_id, payload)
    }

    fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        self.event_bus.subscribe(filter)
    }

    // ---- IPC -------------------------------------------------------------

    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, IpcError> {
        if !self.capabilities.contains(Capability::IpcCall) {
            return Err(IpcError::CapabilityDenied {
                plugin_id: self.plugin_id.clone(),
            });
        }

        let dispatcher = self
            .ipc_dispatcher
            .clone()
            .ok_or(IpcError::DispatcherUnavailable)?;

        let target = target_plugin_id.to_string();
        let command = command_id.to_string();
        let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);

        // Prefer the async path: handlers that perform HTTP / nested IPC must
        // not block the shared plugin-loader mutex, so they expose a Future
        // instead of a sync callable.
        if let Some(fut) = dispatcher.dispatch_async(&target, &command, args.clone()) {
            return match tokio::time::timeout(timeout, fut).await {
                Ok(result) => result,
                Err(_elapsed) => Err(IpcError::Timeout {
                    plugin_id: target,
                    command,
                    timeout_ms,
                }),
            };
        }

        // Fall back to sync dispatch on the blocking thread pool.
        let join = tokio::task::spawn_blocking({
            let target = target.clone();
            let command = command.clone();
            move || dispatcher.dispatch(&target, &command, &args)
        });

        match tokio::time::timeout(timeout, join).await {
            Ok(Ok(result)) => result,
            Ok(Err(_panic)) => Err(IpcError::PluginCrashedDuringCall {
                plugin_id: target,
                command,
            }),
            Err(_elapsed) => Err(IpcError::Timeout {
                plugin_id: target,
                command,
                timeout_ms,
            }),
        }
    }

    // ---- Logging ---------------------------------------------------------

    fn log(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Trace => tracing::trace!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Debug => tracing::debug!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Info  => tracing::info!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Warn  => tracing::warn!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Error => tracing::error!(plugin_id = %self.plugin_id, "{message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::NexusEvent;
    use crate::kv_store::InMemoryKvStore;

    fn make_context(dir: &Path, caps: &[Capability]) -> KernelPluginContext {
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let bus = Arc::new(EventBus::new(16));
        KernelPluginContext::new(
            "com.test.plugin",
            "1.0.0",
            CapabilitySet::from_iter(caps.iter().copied()),
            kv,
            bus,
            dir,
            None,
        )
        .unwrap()
    }

    #[test]
    fn identity_methods_return_correct_values() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        assert_eq!(ctx.plugin_id(), "com.test.plugin");
        assert_eq!(ctx.plugin_version(), "1.0.0");
    }

    #[test]
    fn has_capability_reflects_granted_caps() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::KvRead]);
        assert!(ctx.has_capability(Capability::KvRead));
        assert!(!ctx.has_capability(Capability::KvWrite));
    }

    #[tokio::test]
    async fn kv_requires_capability() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        assert!(ctx.kv_get("key").await.is_err());
        assert!(ctx.kv_set("key", b"val").await.is_err());
    }

    #[tokio::test]
    async fn kv_get_set_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::KvRead, Capability::KvWrite]);
        ctx.kv_set("key", b"hello").await.unwrap();
        let val = ctx.kv_get("key").await.unwrap().unwrap();
        assert_eq!(val, b"hello");
        ctx.kv_delete("key").await.unwrap();
        assert!(ctx.kv_get("key").await.unwrap().is_none());
    }

    #[test]
    fn publish_rejects_namespace_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        let result = ctx.publish("com.other.event", serde_json::json!({}));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn publish_emits_to_subscriber() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        let mut sub = ctx.subscribe(EventFilter::All);

        ctx.publish("com.test.plugin.ping", serde_json::json!({"x": 1})).unwrap();

        let evt = sub.recv().await.unwrap();
        match &evt.event {
            NexusEvent::Custom { type_id, .. } => assert_eq!(type_id, "com.test.plugin.ping"),
            _ => panic!("wrong event"),
        }
    }

    #[tokio::test]
    async fn read_file_denied_without_capability() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        let result = ctx.read_file(&dir.path().join("test.txt")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_write_file_with_capability() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::FsRead, Capability::FsWrite]);
        let file_path = dir.path().join("test.txt");
        ctx.write_file(&file_path, b"hello forge").await.unwrap();
        let contents = ctx.read_file(&file_path).await.unwrap();
        assert_eq!(contents, b"hello forge");
    }

    #[tokio::test]
    async fn confine_path_blocks_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::FsRead]);
        // Try to read /etc/passwd via traversal
        let result = ctx.read_file(Path::new("/etc/passwd")).await;
        assert!(result.is_err());
    }
}
