//! Event types: NexusEvent, EventMetadata, EventFilter, StopReason.

use serde::{Deserialize, Serialize};

/// Why a plugin was stopped. Attached to `NexusEvent::PluginStopped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    /// The user explicitly stopped the plugin via CLI.
    UserRequested,
    /// The plugin is being reloaded from disk (hot-reload).
    HotReload,
    /// The kernel is shutting down.
    Shutdown,
    /// The plugin crashed and is being stopped as part of recovery.
    CrashRecovery,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reason_variants_are_distinct() {
        assert_ne!(StopReason::UserRequested, StopReason::HotReload);
        assert_ne!(StopReason::HotReload, StopReason::Shutdown);
        assert_ne!(StopReason::Shutdown, StopReason::CrashRecovery);
    }

    #[test]
    fn stop_reason_serializes_as_variant_name() {
        let json = serde_json::to_string(&StopReason::HotReload).unwrap();
        assert_eq!(json, "\"HotReload\"");
    }

    #[test]
    fn stop_reason_deserializes_from_variant_name() {
        let reason: StopReason = serde_json::from_str("\"Shutdown\"").unwrap();
        assert_eq!(reason, StopReason::Shutdown);
    }
}
