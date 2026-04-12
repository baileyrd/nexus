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
}
