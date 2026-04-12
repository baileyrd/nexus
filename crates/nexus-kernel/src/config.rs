//! Kernel configuration.

use std::path::PathBuf;

/// Configuration for a Kernel instance.
///
/// Load from disk via `KernelConfig::load`, or construct programmatically
/// (typically via `KernelConfig::for_testing` in tests).
#[derive(Debug, Clone)]
pub struct KernelConfig {
    /// Root directory of the forge (workspace).
    pub forge_root: PathBuf,

    /// Event bus ring buffer capacity. Slow subscribers receive
    /// `RecvError::Lagged(n)` if they fall more than this many events behind.
    pub event_bus_capacity: usize,

    /// Directories to search for plugin manifests. Default:
    /// `[<forge_root>/.nexus/plugins]`.
    pub plugin_search_paths: Vec<PathBuf>,

    /// Enable hot-reload of plugins when their WASM files change on disk.
    pub hot_reload_enabled: bool,
}

impl KernelConfig {
    /// Programmatic construction for tests. Uses defaults for everything
    /// except `forge_root`.
    #[must_use]
    pub fn for_testing(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            ..Self::default()
        }
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            forge_root: PathBuf::from("."),
            event_bus_capacity: 2048,
            plugin_search_paths: vec![],
            hot_reload_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_expected_values() {
        let cfg = KernelConfig::default();
        assert_eq!(cfg.event_bus_capacity, 2048);
        assert!(cfg.hot_reload_enabled);
        assert!(cfg.plugin_search_paths.is_empty());
    }

    #[test]
    fn for_testing_sets_forge_root() {
        let cfg = KernelConfig::for_testing(PathBuf::from("/tmp/test"));
        assert_eq!(cfg.forge_root, PathBuf::from("/tmp/test"));
        assert_eq!(cfg.event_bus_capacity, 2048); // default preserved
    }

    #[test]
    fn config_is_clone() {
        let cfg = KernelConfig::default();
        let _cloned = cfg.clone();
    }
}
