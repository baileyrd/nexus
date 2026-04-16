//! Plugin registry: read-only view of loaded plugins.

use std::collections::HashMap;

use crate::plugin::{PluginInfo, PluginStatus};

/// Read-only snapshot view of plugins loaded in the kernel.
///
/// Currently always empty — the live plugin registry is owned by
/// `nexus-plugins::PluginLoader`. A future refactor will inject a
/// `PluginRegistryReader` so `Kernel::plugins()` delegates to the real loader.
#[derive(Debug, Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginInfo>,
}

impl PluginRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// List all currently loaded plugins. Order is unspecified for M1;
    /// topological ordering is added when `nexus-plugins` lands.
    #[must_use]
    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins.values().cloned().collect()
    }

    /// Look up a plugin by id.
    #[must_use]
    pub fn get(&self, plugin_id: &str) -> Option<PluginInfo> {
        self.plugins.get(plugin_id).cloned()
    }

    /// Count plugins grouped by status.
    #[must_use]
    pub fn count_by_status(&self) -> HashMap<PluginStatus, usize> {
        let mut counts = HashMap::new();
        for info in self.plugins.values() {
            *counts.entry(info.status).or_insert(0) += 1;
        }
        counts
    }

    /// Number of registered plugins.
    #[must_use]
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_has_no_plugins() {
        let reg = PluginRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }
}
