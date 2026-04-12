//! The `Kernel` struct — entry point for the nexus-kernel crate.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config::KernelConfig;
use crate::error::Result;
use crate::event_bus::EventBus;
use crate::plugin_registry::PluginRegistry;

/// The Nexus kernel. Owns the event bus and plugin registry.
///
/// Usage:
/// ```ignore
/// let config = KernelConfig::for_testing(PathBuf::from("/tmp/test"));
/// let kernel = Kernel::new(config)?;
/// kernel.start().await?;
/// // ... do work ...
/// kernel.shutdown().await?;
/// ```
///
/// **Concurrency note:** In PRD 01 scope, the plugin registry has no
/// runtime mutations (no plugins are ever loaded) so it's stored directly
/// without interior mutability. When `nexus-plugins` lands and adds
/// real plugin discovery and hot-reload, it will refactor this to wrap
/// the registry in a `RwLock` or similar. That refactor is a local change
/// to this file — it does not affect the public contract of `plugins()`.
#[derive(Debug)]
pub struct Kernel {
    config: KernelConfig,
    event_bus: Arc<EventBus>,
    plugins: PluginRegistry,
    shutdown_flag: Arc<AtomicBool>,
}

impl Kernel {
    /// Synchronous constructor. Builds the `Kernel` struct and all in-memory
    /// state, but does NOT start background tasks, discover plugins, or emit
    /// events. Call `start()` to bring the kernel up.
    ///
    /// # Errors
    /// Currently infallible in PRD 01 scope (all validation happens earlier
    /// via `KernelConfig::load`). The `Result` return type is preserved for
    /// forward compatibility with future validation.
    pub fn new(config: KernelConfig) -> Result<Self> {
        let event_bus = Arc::new(EventBus::new(config.event_bus_capacity));
        let plugins = PluginRegistry::new();
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        Ok(Self {
            config,
            event_bus,
            plugins,
            shutdown_flag,
        })
    }

    /// Get a handle to the event bus. Used by `nexus-cli` to install event
    /// taps without going through a plugin.
    #[must_use]
    pub fn event_bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.event_bus)
    }

    /// Read-only access to the kernel config.
    #[must_use]
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    /// Start the kernel. Discovers plugins from `config.plugin_search_paths`,
    /// loads them in topological order, and calls their lifecycle hooks.
    ///
    /// In PRD 01 scope, plugin discovery is a no-op (plugins are the
    /// `nexus-plugins` crate's concern). The kernel starts with an empty
    /// plugin set and is ready to accept event bus subscribers.
    ///
    /// # Errors
    /// Returns `Error::Plugin` if any plugin fails to load or initialize.
    /// In PRD 01 scope, this cannot happen (no plugins are loaded).
    pub async fn start(&self) -> Result<()> {
        tracing::info!(
            forge_root = ?self.config.forge_root,
            event_bus_capacity = self.config.event_bus_capacity,
            "nexus kernel starting"
        );

        // Plugin discovery is a no-op in PRD 01 scope.
        // nexus-plugins will fill this in when it lands.
        tracing::debug!("plugin discovery not yet implemented; starting with empty plugin set");

        tracing::info!("nexus kernel started");
        Ok(())
    }

    /// Graceful shutdown. Stops all plugins in reverse topological order,
    /// drains the event bus, flushes the audit log, closes DB connections.
    /// Idempotent — safe to call twice.
    ///
    /// In PRD 01 scope, shutdown flips a flag and returns. Real drain
    /// behavior fills in when `nexus-plugins` and `nexus-storage` land.
    ///
    /// # Errors
    /// Returns `Error::Plugin` if any plugin fails to stop. In PRD 01
    /// scope, this cannot happen (no plugins are loaded).
    pub async fn shutdown(&self) -> Result<()> {
        // Flip the shutdown flag. Idempotent: subsequent calls see the flag
        // already set and short-circuit.
        let was_already_shutdown =
            self.shutdown_flag.swap(true, std::sync::atomic::Ordering::SeqCst);

        if was_already_shutdown {
            tracing::debug!("nexus kernel shutdown called on already-shutdown kernel; no-op");
            return Ok(());
        }

        tracing::info!("nexus kernel shutting down");

        // In PRD 01 scope, nothing to drain. nexus-plugins and nexus-storage
        // will fill in real drain logic when they land.
        tracing::debug!("no plugins to stop; no storage to flush");

        tracing::info!("nexus kernel shutdown complete");
        Ok(())
    }

    /// Get a read-only handle to the plugin registry. Used by `nexus-cli` for
    /// introspection commands like `nexus plugin list`.
    ///
    /// Synchronous accessor in PRD 01 scope. When `nexus-plugins` adds
    /// runtime mutations, this signature may change to return a
    /// `RwLockReadGuard` — a refactor local to this file.
    #[must_use]
    pub fn plugins(&self) -> &PluginRegistry {
        &self.plugins
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn new_succeeds_with_default_config() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-kernel-test"));
        let kernel = Kernel::new(config).unwrap();
        assert_eq!(kernel.config().forge_root, PathBuf::from("/tmp/nexus-kernel-test"));
    }

    #[test]
    fn event_bus_handle_is_clonable_arc() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp"));
        let kernel = Kernel::new(config).unwrap();
        let bus1 = kernel.event_bus();
        let bus2 = kernel.event_bus();
        // Both are Arc clones pointing at the same bus
        assert_eq!(Arc::as_ptr(&bus1), Arc::as_ptr(&bus2));
    }

    #[tokio::test]
    async fn start_succeeds_with_empty_plugin_set() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-start-test"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
    }

    #[tokio::test]
    async fn start_is_idempotent_across_multiple_calls() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-start-idem"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
        // Calling start again should not fail in PRD 01 scope.
        kernel.start().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_succeeds_on_fresh_kernel() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-shutdown-test"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
        kernel.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-shutdown-idem"));
        let kernel = Kernel::new(config).unwrap();
        kernel.start().await.unwrap();
        kernel.shutdown().await.unwrap();
        kernel.shutdown().await.unwrap();  // no panic, no error
    }

    #[test]
    fn plugins_accessor_returns_empty_registry_before_start() {
        let config = KernelConfig::for_testing(PathBuf::from("/tmp/nexus-plugins-accessor"));
        let kernel = Kernel::new(config).unwrap();
        let registry = kernel.plugins();
        assert!(registry.is_empty());
    }
}
