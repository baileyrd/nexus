//! Plugin-observable error types — the stable error surface plugins see.
//!
//! These are the error variants that cross the kernel/plugin boundary. Kernel-
//! internal errors (config, KV backend failures) are not exposed here.

/// Errors from IPC calls between plugins. Stable across kernel versions.
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

    /// The kernel was built without an IPC dispatcher configured.
    #[error("no IPC dispatcher configured on this kernel")]
    DispatcherUnavailable,

    /// A re-entrant or circular IPC call was detected.
    #[error("re-entrant IPC call to '{plugin_id}'.'{command}'")]
    ReentrantCall {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
    },
}

/// Event bus errors visible to plugins.
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    /// The event bus has been shut down.
    #[error("event bus is closed")]
    Closed,

    /// A plugin tried to publish a `Custom` event whose `type_id` does not
    /// start with the plugin's own id (anti-spoofing enforcement).
    #[error("custom event rejected: type_id '{type_id}' does not start with emitting plugin id '{plugin_id}'")]
    TypeIdNamespaceMismatch {
        /// Plugin that attempted to publish.
        plugin_id: String,
        /// The rejected `type_id`.
        type_id: String,
    },

    /// A plugin tried to publish a kernel-owned event variant.
    #[error("plugins cannot publish kernel events; only NexusEvent::Custom is allowed from plugins")]
    PluginPublishingKernelEvent,
}

/// Capability system errors visible to plugins.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_error_timeout_displays_ms() {
        let err = IpcError::Timeout {
            plugin_id: "com.test".to_string(),
            command: "ping".to_string(),
            timeout_ms: 5000,
        };
        let msg = format!("{err}");
        assert!(msg.contains("5000"));
    }

    #[test]
    fn bus_error_namespace_mismatch_displays_both() {
        let err = BusError::TypeIdNamespaceMismatch {
            plugin_id: "com.foo".to_string(),
            type_id: "com.bar.event".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("com.foo"));
        assert!(msg.contains("com.bar.event"));
    }

    #[test]
    fn ipc_error_is_clone() {
        let e = IpcError::DispatcherUnavailable;
        let _e2 = e.clone();
    }
}
