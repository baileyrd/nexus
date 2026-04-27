//! The `Kernel` struct — entry point for the nexus-kernel crate.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config::KernelConfig;
use crate::error::Result;
use crate::event_bus::EventBus;
use crate::kv_store::KvStore;

/// The Nexus kernel. Owns the event bus, KV store, and lifecycle flag.
///
/// Usage:
/// ```ignore
/// let config = KernelConfig::for_testing(PathBuf::from("/tmp/test"));
/// let kv = Arc::new(nexus_kv::InMemoryKvStore::new());
/// let kernel = Kernel::new(config, kv)?;
/// kernel.start().await?;
/// // ... do work ...
/// kernel.shutdown().await?;
/// ```
///
/// **Plugin tracking lives elsewhere.** The authoritative map of loaded
/// plugins is `nexus_plugins::PluginLoader::loaded`. The kernel does not
/// hold a parallel registry — see OI-13 for the cleanup that removed the
/// always-empty `Kernel::plugins()` accessor.
#[derive(Debug)]
pub struct Kernel {
    config: KernelConfig,
    event_bus: Arc<EventBus>,
    kv_store: Arc<dyn KvStore>,
    shutdown_flag: Arc<AtomicBool>,
}

impl Kernel {
    /// Synchronous constructor. Builds the `Kernel` struct and all in-memory
    /// state, but does NOT start background tasks, discover plugins, or emit
    /// events. Call `start()` to bring the kernel up.
    ///
    /// `kv_store` is injected — pick a backend from `nexus-kv`
    /// ([`nexus_kv::SqliteKvStore`](../../nexus_kv/struct.SqliteKvStore.html)
    /// for the real runtime,
    /// [`nexus_kv::InMemoryKvStore`](../../nexus_kv/struct.InMemoryKvStore.html)
    /// for tests).
    ///
    /// # Errors
    /// Currently infallible in PRD 01 scope (all validation happens earlier
    /// via `KernelConfig::load`). The `Result` return type is preserved for
    /// forward compatibility with future validation.
    pub fn new(config: KernelConfig, kv_store: Arc<dyn KvStore>) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new(config.event_bus_capacity));
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        Ok(Self {
            config,
            event_bus,
            kv_store,
            shutdown_flag,
        })
    }

    /// Get a handle to the event bus. Used by `nexus-cli` to install event
    /// taps without going through a plugin.
    #[must_use]
    pub fn event_bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.event_bus)
    }

    /// Get a handle to the kernel KV store as a trait object.
    ///
    /// Used by `nexus-plugins` to inject storage into plugin sandboxes.
    #[must_use]
    pub fn kv_store(&self) -> Arc<dyn KvStore> {
        Arc::clone(&self.kv_store)
    }

    /// Read-only access to the kernel config.
    #[must_use]
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    /// Emit the "kernel online" trace marker and return.
    ///
    /// The kernel deliberately does **not** own plugin lifecycle — that
    /// lives in `PluginManager::load_all` (the canonical entry point for
    /// discovering and starting plugins). This method is a logging hook
    /// retained for PRD-01 compatibility; callers should continue to
    /// drive plugin lifecycle through `PluginManager`.
    ///
    /// # Errors
    /// Infallible today. The `Result` return is preserved for forward
    /// compatibility.
    #[allow(clippy::unused_async)]
    pub async fn start(&self) -> Result<()> {
        tracing::info!(
            forge_root = ?self.config.forge_root,
            event_bus_capacity = self.config.event_bus_capacity,
            "nexus kernel online (plugin lifecycle owned by PluginManager)"
        );
        Ok(())
    }

    /// Flip the kernel shutdown flag. Idempotent.
    ///
    /// Plugins are stopped by `PluginManager::shutdown` (which drains them
    /// in reverse-registration order). This method only toggles the
    /// internal shutdown sentinel used by long-running tasks that hold an
    /// `Arc<AtomicBool>` reference to it.
    ///
    /// # Errors
    /// Infallible today. The `Result` return is preserved for forward
    /// compatibility.
    #[allow(clippy::unused_async)]
    pub async fn shutdown(&self) -> Result<()> {
        let was_already_shutdown =
            self.shutdown_flag.swap(true, std::sync::atomic::Ordering::SeqCst);
        if was_already_shutdown {
            tracing::debug!("nexus kernel shutdown already signalled; no-op");
            return Ok(());
        }
        tracing::info!("nexus kernel shutdown signalled");
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv_store::InMemoryKvStore;
    use std::path::PathBuf;

    fn kv() -> Arc<dyn KvStore> {
        Arc::new(InMemoryKvStore::new())
    }

    #[test]
    fn new_succeeds_with_default_config() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-kernel-test"));
        let kernel = Kernel::new(config, kv()).unwrap();
        assert_eq!(kernel.config().forge_root, PathBuf::from("/tmp/nexus-kernel-test"));
    }

    #[test]
    fn event_bus_handle_is_clonable_arc() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp"));
        let kernel = Kernel::new(config, kv()).unwrap();
        let bus1 = kernel.event_bus();
        let bus2 = kernel.event_bus();
        // Both are Arc clones pointing at the same bus
        assert_eq!(Arc::as_ptr(&bus1), Arc::as_ptr(&bus2));
    }

    #[tokio::test]
    async fn start_succeeds_with_empty_plugin_set() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-start-test"));
        let kernel = Kernel::new(config, kv()).unwrap();
        kernel.start().await.unwrap();
    }

    #[tokio::test]
    async fn start_is_idempotent_across_multiple_calls() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-start-idem"));
        let kernel = Kernel::new(config, kv()).unwrap();
        kernel.start().await.unwrap();
        // Calling start again should not fail in PRD 01 scope.
        kernel.start().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_succeeds_on_fresh_kernel() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-shutdown-test"));
        let kernel = Kernel::new(config, kv()).unwrap();
        kernel.start().await.unwrap();
        kernel.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-shutdown-idem"));
        let kernel = Kernel::new(config, kv()).unwrap();
        kernel.start().await.unwrap();
        kernel.shutdown().await.unwrap();
        kernel.shutdown().await.unwrap();  // no panic, no error
    }

}
