//! Event types: NexusEvent, EventMetadata, EventFilter, StopReason.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::capability::Capability;

/// All events the Nexus kernel knows about.
///
/// This is a closed enum for kernel-owned events plus a single `Custom`
/// variant for plugin-emitted signals. Each phase of the roadmap adds
/// variants here when it reaches its milestone. M1 includes storage, plugin
/// lifecycle, capability, and indexing events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NexusEvent {
    // ---- M1: storage events ----

    /// A file was created in the forge.
    FileCreated {
        /// Path of the created file, relative to forge root.
        path: PathBuf,
        /// Content hash (SHA-256 hex) of the file.
        content_hash: String,
    },
    /// A file's contents changed.
    FileModified {
        /// Path of the modified file.
        path: PathBuf,
        /// New content hash.
        content_hash: String,
    },
    /// A file was deleted.
    FileDeleted {
        /// Path of the deleted file.
        path: PathBuf,
    },
    /// A file was renamed (detected via hash match within the debounce window).
    FileRenamed {
        /// Old path.
        from: PathBuf,
        /// New path.
        to: PathBuf,
        /// Content hash (unchanged across rename).
        content_hash: String,
    },

    // ---- M1: plugin lifecycle events ----

    /// A plugin has been loaded from disk and its manifest parsed.
    PluginLoaded {
        /// Plugin identifier (reverse-DNS).
        plugin_id: String,
        /// Plugin version from the manifest.
        version: String,
    },
    /// A plugin has started successfully.
    PluginStarted {
        /// Plugin identifier.
        plugin_id: String,
    },
    /// A plugin has been stopped.
    PluginStopped {
        /// Plugin identifier.
        plugin_id: String,
        /// Why the plugin was stopped.
        reason: StopReason,
    },
    /// A plugin crashed during execution.
    PluginCrashed {
        /// Plugin identifier.
        plugin_id: String,
        /// Description of what went wrong.
        error: String,
    },

    // ---- M1: capability lifecycle events ----

    /// A plugin was granted a capability.
    CapabilityGranted {
        /// Plugin identifier.
        plugin_id: String,
        /// Which capability was granted.
        capability: Capability,
    },
    /// A plugin's capability request was denied.
    CapabilityDenied {
        /// Plugin identifier.
        plugin_id: String,
        /// Which capability was denied.
        capability: Capability,
    },

    // ---- M1: indexing events ----

    /// Storage engine has begun indexing.
    IndexingStarted {
        /// Total number of files the indexer will process.
        total_files: usize,
    },
    /// Storage engine indexing progress update.
    IndexingProgress {
        /// Files processed so far.
        files_processed: usize,
        /// Total files in the batch.
        total_files: usize,
    },
    /// Storage engine indexing completed.
    IndexingCompleted {
        /// Wall-clock duration of the indexing pass, in milliseconds.
        duration_ms: u64,
    },

    // ---- Plugin-emitted custom events ----

    /// A plugin-emitted signal. Anti-spoofing enforced at publish time:
    /// `type_id` must start with the emitting plugin's id, and
    /// `emitting_plugin` is set by the kernel (not the plugin).
    Custom {
        /// Namespaced event type (reverse-DNS).
        type_id: String,
        /// The plugin that emitted this event. Set by the kernel.
        emitting_plugin: String,
        /// Arbitrary payload. Plugins serialize/deserialize with their own types.
        payload: serde_json::Value,
    },
}

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

    #[test]
    fn file_created_event_constructs_and_serializes() {
        let event = NexusEvent::FileCreated {
            path: PathBuf::from("welcome.md"),
            content_hash: "abc123".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"FileCreated\""));
        assert!(json.contains("welcome.md"));
    }

    #[test]
    fn plugin_stopped_event_includes_reason() {
        let event = NexusEvent::PluginStopped {
            plugin_id: "com.test".to_string(),
            reason: StopReason::HotReload,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("HotReload"));
    }

    #[test]
    fn custom_event_carries_type_id_and_payload() {
        let event = NexusEvent::Custom {
            type_id: "com.example.test.ping".to_string(),
            emitting_plugin: "com.example.test".to_string(),
            payload: serde_json::json!({"hello": "world"}),
        };
        match event {
            NexusEvent::Custom { type_id, .. } => assert_eq!(type_id, "com.example.test.ping"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn file_renamed_is_single_event_not_two() {
        // Confirms the enum shape: one event carries both from and to,
        // not a Delete + Create pair.
        let event = NexusEvent::FileRenamed {
            from: PathBuf::from("old.md"),
            to: PathBuf::from("new.md"),
            content_hash: "abc".to_string(),
        };
        match event {
            NexusEvent::FileRenamed { from, to, .. } => {
                assert_eq!(from, PathBuf::from("old.md"));
                assert_eq!(to, PathBuf::from("new.md"));
            }
            _ => panic!("wrong variant"),
        }
    }
}
