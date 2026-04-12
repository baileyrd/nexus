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

/// Filter applied to an event subscription. Events not matching the filter
/// are silently skipped inside the subscription's `recv()` call.
#[derive(Debug, Clone)]
pub enum EventFilter {
    /// Match every event on the bus. High-traffic; intended for debug/tracing.
    All,
    /// Match a single kernel-event variant by its name (e.g., `"FileCreated"`).
    Variant(&'static str),
    /// Match `NexusEvent::Custom` events whose `type_id` starts with this prefix.
    CustomPrefix(String),
    /// Match exactly one `NexusEvent::Custom` `type_id`.
    CustomExact(String),
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

    #[test]
    fn event_filter_is_clone() {
        let f1 = EventFilter::Variant("FileCreated");
        let _f2 = f1.clone();
    }

    #[test]
    fn custom_prefix_stores_string() {
        let filter = EventFilter::CustomPrefix("com.example.".to_string());
        match filter {
            EventFilter::CustomPrefix(p) => assert_eq!(p, "com.example."),
            _ => panic!("wrong variant"),
        }
    }
}
