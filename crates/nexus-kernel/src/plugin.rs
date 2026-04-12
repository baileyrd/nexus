//! Plugin-related types: lifecycle trait, trust levels, status, info.

use crate::capability::CapabilitySet;

/// Trust level declared by a plugin in its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Core plugins (authored or explicitly blessed); any capability allowed.
    Core,
    /// Community plugins; HIGH-risk capabilities require install-time approval.
    Community,
}

/// Plugin runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginStatus {
    /// Loaded from disk, manifest parsed, not yet initialized.
    Loaded,
    /// `on_init` completed successfully.
    Initialized,
    /// `on_start` completed; plugin is running.
    Running,
    /// `on_stop` completed; plugin is no longer receiving events.
    Stopped,
    /// Plugin crashed (error-path sink).
    Crashed,
}

/// Public view of a loaded plugin's identity and state.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin identifier (reverse-DNS).
    pub id: String,
    /// Human-readable display name from the manifest.
    pub name: String,
    /// Version string from the manifest.
    pub version: String,
    /// Trust level declared in the manifest.
    pub trust_level: TrustLevel,
    /// Current runtime status.
    pub status: PluginStatus,
    /// Capabilities granted to this plugin at load time.
    pub capabilities: CapabilitySet,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    #[test]
    fn plugin_info_constructs_with_all_fields() {
        let info = PluginInfo {
            id: "com.example.test".to_string(),
            name: "Test".to_string(),
            version: "0.1.0".to_string(),
            trust_level: TrustLevel::Core,
            status: PluginStatus::Running,
            capabilities: CapabilitySet::from_iter([Capability::FsRead]),
        };
        assert_eq!(info.id, "com.example.test");
        assert_eq!(info.trust_level, TrustLevel::Core);
        assert_eq!(info.status, PluginStatus::Running);
        assert!(info.capabilities.contains(Capability::FsRead));
    }

    #[test]
    fn trust_level_variants_are_distinct() {
        assert_ne!(TrustLevel::Core, TrustLevel::Community);
    }

    #[test]
    fn plugin_status_is_copy_and_eq() {
        let a = PluginStatus::Running;
        let b = a;
        assert_eq!(a, b);
    }
}
