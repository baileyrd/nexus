//! Plugin registry: read-only view of loaded plugins.

use std::collections::HashMap;

use crate::plugin::{PluginInfo, PluginStatus};

/// Read-only view of plugins loaded in the kernel.
///
/// Populated by the kernel during plugin discovery (not implemented in PRD 01
/// scope — the registry is empty until `nexus-plugins` lands). Exposed through
/// `Kernel::plugins()` so `nexus-cli` can implement introspection commands
/// like `nexus plugin list`.
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

    /// Insert or update a plugin info entry. Not part of the public contract —
    /// `nexus-plugins` will call this during load.
    pub(crate) fn upsert(&mut self, info: PluginInfo) {
        self.plugins.insert(info.id.clone(), info);
    }

    /// Remove a plugin from the registry. Not part of the public contract.
    pub(crate) fn remove(&mut self, plugin_id: &str) -> Option<PluginInfo> {
        self.plugins.remove(plugin_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;
    use crate::plugin::{PluginStatus, TrustLevel};

    fn sample_info(id: &str, status: PluginStatus) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1.0".to_string(),
            trust_level: TrustLevel::Core,
            status,
            capabilities: CapabilitySet::empty(),
        }
    }

    #[test]
    fn empty_registry_has_no_plugins() {
        let reg = PluginRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list().is_empty());
    }

    #[test]
    fn upsert_and_get_roundtrip() {
        let mut reg = PluginRegistry::new();
        reg.upsert(sample_info("com.test", PluginStatus::Running));
        let got = reg.get("com.test").unwrap();
        assert_eq!(got.id, "com.test");
    }

    #[test]
    fn count_by_status_groups_correctly() {
        let mut reg = PluginRegistry::new();
        reg.upsert(sample_info("a", PluginStatus::Running));
        reg.upsert(sample_info("b", PluginStatus::Running));
        reg.upsert(sample_info("c", PluginStatus::Stopped));

        let counts = reg.count_by_status();
        assert_eq!(counts.get(&PluginStatus::Running), Some(&2));
        assert_eq!(counts.get(&PluginStatus::Stopped), Some(&1));
    }

    #[test]
    fn remove_returns_the_removed_info() {
        let mut reg = PluginRegistry::new();
        reg.upsert(sample_info("a", PluginStatus::Running));
        let removed = reg.remove("a").unwrap();
        assert_eq!(removed.id, "a");
        assert!(reg.is_empty());
    }
}
