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

/// Placeholder for PluginError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Placeholder variant, replaced in Task 15.
    #[error("plugin error placeholder")]
    Placeholder,
}

/// Placeholder for CapabilityError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    /// Placeholder variant, replaced in Task 15.
    #[error("capability error placeholder")]
    Placeholder,
}

/// Placeholder for IpcError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Placeholder variant, replaced in Task 15.
    #[error("IPC error placeholder")]
    Placeholder,
}

/// Placeholder for KvError (filled in in Task 15).
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    /// Placeholder variant, replaced in Task 15.
    #[error("KV error placeholder")]
    Placeholder,
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
        /// The rejected type_id.
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
}
