//! Error types for the kernel crate.
//!
//! Organization: top-level `Error` enum with `#[from]` wrapping of
//! per-subsystem sub-enums. Narrow APIs can return narrow types directly.

use std::path::PathBuf;

/// Top-level result type for `nexus-kernel`.
pub type Result<T> = std::result::Result<T, Error>;

/// Top-level error type for `nexus-kernel`. Wraps per-subsystem errors
/// plus `std::io::Error` for convenience.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Plugin lifecycle error.
    #[error(transparent)]
    Plugin(#[from] PluginError),

    /// Capability system error.
    #[error(transparent)]
    Capability(#[from] CapabilityError),

    /// IPC call error.
    #[error(transparent)]
    Ipc(#[from] IpcError),

    /// Event bus error.
    #[error(transparent)]
    Bus(#[from] BusError),

    /// KV store error.
    #[error(transparent)]
    Kv(#[from] KvError),

    /// Configuration load error.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Errors related to plugin lifecycle, loading, and dependency resolution.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Plugin failed to load from disk.
    #[error("plugin '{plugin_id}' failed to load: {reason}")]
    LoadFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable failure reason.
        reason: String,
    },

    /// Plugin's `on_init` hook failed.
    #[error("plugin '{plugin_id}' failed to initialize: {reason}")]
    InitFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin's `on_start` hook failed.
    #[error("plugin '{plugin_id}' failed to start: {reason}")]
    StartFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin's `on_stop` hook failed.
    #[error("plugin '{plugin_id}' failed to stop: {reason}")]
    StopFailed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin crashed during execution.
    #[error("plugin '{plugin_id}' crashed: {reason}")]
    Crashed {
        /// Plugin id.
        plugin_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Plugin panicked during a lifecycle phase.
    #[error("plugin '{plugin_id}' panicked during {phase}")]
    Panicked {
        /// Plugin id.
        plugin_id: String,
        /// Which phase (e.g., "init", "start", "stop").
        phase: &'static str,
    },

    /// Dependency cycle detected among plugins.
    #[error("dependency cycle among plugins: {plugins:?}")]
    DependencyCycle {
        /// Plugin ids involved in the cycle.
        plugins: Vec<String>,
    },

    /// A plugin's required dependency is not loaded.
    #[error("plugin '{plugin_id}' missing required dependency '{missing}'")]
    MissingDependency {
        /// Plugin that has the missing dependency.
        plugin_id: String,
        /// The dependency that wasn't found.
        missing: String,
    },

    /// A plugin's required dependency is the wrong version.
    #[error("plugin '{plugin_id}' dependency '{missing}' version mismatch: required {required}, found {found}")]
    DependencyVersionMismatch {
        /// Plugin with the version mismatch.
        plugin_id: String,
        /// Dependency name.
        missing: String,
        /// Version constraint from the manifest.
        required: String,
        /// Actual version found on disk.
        found: String,
    },

    /// Two plugins declared the same id.
    #[error("duplicate plugin id '{plugin_id}'")]
    DuplicatePluginId {
        /// The duplicated id.
        plugin_id: String,
    },

    /// Plugin lookup by id failed.
    #[error("plugin '{plugin_id}' not found")]
    NotFound {
        /// The id that wasn't found.
        plugin_id: String,
    },
}

/// Errors related to the capability system.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    /// A plugin requested a capability it was not granted.
    #[error("capability '{cap:?}' denied to plugin '{plugin_id}'")]
    Denied {
        /// Plugin id.
        plugin_id: String,
        /// The denied capability.
        cap: crate::capability::Capability,
    },

    /// A manifest contained an unrecognized capability string.
    #[error("unknown capability string '{0}'")]
    UnknownString(String),
}

/// Errors from IPC calls between plugins.
#[derive(Debug, Clone, thiserror::Error)]
pub enum IpcError {
    /// The target plugin is not loaded.
    #[error("target plugin '{plugin_id}' not found")]
    PluginNotFound {
        /// The target plugin id.
        plugin_id: String,
    },

    /// The target plugin doesn't register that command.
    #[error("command '{command}' not found on plugin '{plugin_id}'")]
    CommandNotFound {
        /// The target plugin id.
        plugin_id: String,
        /// The requested command id.
        command: String,
    },

    /// The IPC call timed out.
    #[error("IPC call to '{plugin_id}'.'{command}' timed out after {timeout_ms}ms")]
    Timeout {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
        /// Timeout that was exceeded.
        timeout_ms: u64,
    },

    /// The target plugin crashed during the IPC call.
    #[error("plugin '{plugin_id}' crashed during IPC call to '{command}'")]
    PluginCrashedDuringCall {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
    },

    /// Failed to serialize the argument payload.
    #[error("IPC argument serialization failed: {reason}")]
    SerializationFailed {
        /// Reason from the serializer.
        reason: String,
    },

    /// Failed to deserialize the return value.
    #[error("IPC return value deserialization failed: {reason}")]
    DeserializationFailed {
        /// Reason from the deserializer.
        reason: String,
    },

    /// The caller lacks the `ipc.call` capability.
    #[error("capability denied for {plugin_id}: ipc.call")]
    CapabilityDenied {
        /// The caller plugin id.
        plugin_id: String,
    },

    /// The kernel was built without an [`crate::IpcDispatcher`], so IPC calls
    /// cannot be routed. Typical in unit tests or bare kernels.
    #[error("no IPC dispatcher configured on this kernel")]
    DispatcherUnavailable,

    /// A re-entrant or circular IPC call was detected — the target plugin's
    /// backend mutex was already locked by the current call chain.
    #[error("re-entrant IPC call to '{plugin_id}'.'{command}'")]
    ReentrantCall {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
    },
}

/// Errors from the KV store.
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    /// Key not found in the store.
    #[error("key '{key}' not found")]
    NotFound {
        /// The missing key.
        key: String,
    },

    /// Generic KV store failure (wraps the storage backend error).
    #[error("KV store backend error: {reason}")]
    BackendError {
        /// Human-readable reason from the backend.
        reason: String,
    },
}

/// Event bus errors.
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    /// The event bus has been shut down; publishing or subscribing fails.
    #[error("event bus is closed")]
    Closed,

    /// A plugin tried to publish a `Custom` event whose `type_id` does not
    /// start with the plugin's own id.
    #[error("custom event rejected: type_id '{type_id}' does not start with emitting plugin id '{plugin_id}'")]
    TypeIdNamespaceMismatch {
        /// Plugin that attempted to publish.
        plugin_id: String,
        /// The rejected `type_id`.
        type_id: String,
    },

    /// A plugin tried to publish a kernel-owned event variant
    /// (plugins can only publish `NexusEvent::Custom`).
    #[error("plugins cannot publish kernel events; only NexusEvent::Custom is allowed from plugins")]
    PluginPublishingKernelEvent,
}

/// Errors from receiving events on a subscription.
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// The subscriber fell behind by `n` events; those events are lost.
    /// The subscription is still alive; call `recv` again to keep going.
    #[error("subscriber lagged by {0} events (events lost)")]
    Lagged(u64),

    /// The event bus has been shut down. The subscription is dead.
    #[error("event bus is closed")]
    Closed,
}

/// Configuration load errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Config file not found.
    #[error("config file not found at '{path}'")]
    NotFound {
        /// Path that was checked.
        path: PathBuf,
    },

    /// Config file exists but is invalid.
    #[error("invalid config at '{path}': {reason}")]
    Invalid {
        /// Path of the invalid file.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },

    /// TOML parse error with source location.
    #[error("TOML parse error in '{path}': {source}")]
    TomlParse {
        /// Path of the file that failed to parse.
        path: PathBuf,
        /// The underlying TOML parse error.
        #[source]
        source: toml::de::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_error_wraps_bus_error() {
        let bus_err = BusError::Closed;
        let kernel_err: Error = bus_err.into();
        assert!(matches!(kernel_err, Error::Bus(BusError::Closed)));
    }

    #[test]
    fn config_error_display_includes_path() {
        let err = ConfigError::NotFound {
            path: PathBuf::from("/missing/config.toml"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/missing/config.toml"));
    }

    #[test]
    fn recv_error_lagged_carries_count() {
        let err = RecvError::Lagged(42);
        let msg = format!("{err}");
        assert!(msg.contains("42"));
    }

    #[test]
    fn bus_error_type_id_mismatch_displays_both_fields() {
        let err = BusError::TypeIdNamespaceMismatch {
            plugin_id: "com.foo".to_string(),
            type_id: "com.bar.event".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("com.foo"));
        assert!(msg.contains("com.bar.event"));
    }

    #[test]
    fn plugin_error_crashed_displays_id_and_reason() {
        let err = PluginError::Crashed {
            plugin_id: "com.test".to_string(),
            reason: "segfault".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("com.test"));
        assert!(msg.contains("segfault"));
    }

    #[test]
    fn capability_error_denied_debug_prints_variant() {
        let err = CapabilityError::Denied {
            plugin_id: "com.test".to_string(),
            cap: crate::capability::Capability::FsRead,
        };
        let msg = format!("{err}");
        assert!(msg.contains("FsRead"));
        assert!(msg.contains("com.test"));
    }

    #[test]
    fn ipc_error_timeout_includes_duration() {
        let err = IpcError::Timeout {
            plugin_id: "com.test".to_string(),
            command: "ping".to_string(),
            timeout_ms: 5000,
        };
        let msg = format!("{err}");
        assert!(msg.contains("5000"));
    }

    #[test]
    fn kv_error_not_found_shows_key() {
        let err = KvError::NotFound {
            key: "state".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("state"));
    }

    #[test]
    fn plugin_error_dep_cycle_lists_plugins() {
        let err = PluginError::DependencyCycle {
            plugins: vec!["a".to_string(), "b".to_string()],
        };
        let msg = format!("{err}");
        assert!(msg.contains('a'));
        assert!(msg.contains('b'));
    }
}
