//! Plugin-observable error types — the stable error surface plugins see.
//!
//! These are the error variants that cross the kernel/plugin boundary. Kernel-
//! internal errors (config, KV backend failures) are not exposed here.

/// Errors from IPC calls between plugins. Stable across kernel versions.
#[derive(Debug, Clone, thiserror::Error)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/"))]
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
    #[error("plugin '{plugin_id}' crashed during IPC call to '{command}'{}", if reason.is_empty() { String::new() } else { format!(": {reason}") })]
    PluginCrashedDuringCall {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
        /// Underlying execution reason, if available. Empty for true panics.
        reason: String,
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

    /// The IPC call was cancelled by the caller (or an ancestor)
    /// firing the cancellation token that was active for this
    /// dispatch. Handlers that observed the token mid-flight via
    /// `nexus_kernel::ipc_cancel_token()` returned this themselves;
    /// the kernel synthesises it as the wait-side result when the
    /// token fires before the handler future resolves and the
    /// handler did not opt in to cancellation.
    #[error("IPC call to '{plugin_id}'.'{command}' was cancelled")]
    Cancelled {
        /// The target plugin id.
        plugin_id: String,
        /// The command id.
        command: String,
    },
}

/// Coarse classification of an [`IpcError`] suitable for cross-boundary
/// branching by frontend / plugin code.
///
/// The Rust `IpcError` enum is the source of truth, but it has shape that
/// doesn't survive a JSON serialization round-trip (e.g. nested strings,
/// borrowed displays). [`IpcErrorEnvelope`] flattens the variant down to a
/// stable category so callers don't have to string-sniff `Display` output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/"))]
pub enum IpcErrorKind {
    /// IPC call exceeded its deadline.
    Timeout,
    /// The target plugin panicked while servicing the call.
    PluginCrashed,
    /// The caller lacks the required capability.
    CapabilityDenied,
    /// The kernel could not route the call (no plugin / command / dispatcher,
    /// or a re-entrant call).
    DispatchFailed,
    /// (De)serialization of the IPC payload failed.
    Serialization,
    /// The IPC call was cancelled cooperatively via the dispatch's
    /// `CancellationToken`. Distinct from `Timeout` (deadline exceeded)
    /// and `DispatchFailed` (could not route).
    Cancelled,
    /// Variant the envelope mapper didn't recognize. Reserved for future
    /// `IpcError` additions so old shells still surface the failure.
    Unknown,
}

/// Wire-stable envelope describing an [`IpcError`].
///
/// Returned as the `Err` payload of `kernel_invoke` so the shell / frontend
/// can branch on `kind` (and respect `retryable`) without reflecting on the
/// underlying Rust enum.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/"))]
pub struct IpcErrorEnvelope {
    /// Coarse error category — see [`IpcErrorKind`].
    pub kind: IpcErrorKind,
    /// The plugin id involved in the failed call. Empty string when the
    /// underlying variant doesn't carry one (e.g. `SerializationFailed`).
    pub plugin_id: String,
    /// The command id involved in the failed call. Empty string when the
    /// underlying variant doesn't carry one (e.g. `PluginNotFound`).
    pub command: String,
    /// Human-readable rendering of the original error (`thiserror::Display`).
    pub message: String,
    /// Whether the caller may retry the same call and reasonably expect a
    /// different outcome. Only `Timeout` is currently retryable.
    pub retryable: bool,
}

impl IpcErrorEnvelope {
    /// Map an [`IpcError`] into a wire-stable envelope.
    ///
    /// Variants that don't carry plugin/command context produce empty
    /// strings for those fields. Use
    /// [`IpcErrorEnvelope::from_ipc_error_in_context`] when the caller has
    /// fallback identifiers (e.g. the args passed to `kernel_invoke`).
    #[must_use]
    pub fn from_ipc_error(err: &IpcError) -> Self {
        let message = format!("{err}");
        match err {
            IpcError::Timeout {
                plugin_id, command, ..
            } => Self {
                kind: IpcErrorKind::Timeout,
                plugin_id: plugin_id.clone(),
                command: command.clone(),
                message,
                retryable: true,
            },
            IpcError::PluginNotFound { plugin_id } => Self {
                kind: IpcErrorKind::DispatchFailed,
                plugin_id: plugin_id.clone(),
                command: String::new(),
                message,
                retryable: false,
            },
            IpcError::CapabilityDenied { plugin_id } => Self {
                kind: IpcErrorKind::CapabilityDenied,
                plugin_id: plugin_id.clone(),
                command: String::new(),
                message,
                retryable: false,
            },
            IpcError::PluginCrashedDuringCall { plugin_id, command, .. } => Self {
                kind: IpcErrorKind::PluginCrashed,
                plugin_id: plugin_id.clone(),
                command: command.clone(),
                message,
                retryable: false,
            },
            IpcError::SerializationFailed { .. } | IpcError::DeserializationFailed { .. } => Self {
                kind: IpcErrorKind::Serialization,
                plugin_id: String::new(),
                command: String::new(),
                message,
                retryable: false,
            },
            IpcError::DispatcherUnavailable => Self {
                kind: IpcErrorKind::DispatchFailed,
                plugin_id: String::new(),
                command: String::new(),
                message,
                retryable: false,
            },
            IpcError::CommandNotFound { plugin_id, command }
            | IpcError::ReentrantCall { plugin_id, command } => Self {
                kind: IpcErrorKind::DispatchFailed,
                plugin_id: plugin_id.clone(),
                command: command.clone(),
                message,
                retryable: false,
            },
            IpcError::Cancelled { plugin_id, command } => Self {
                kind: IpcErrorKind::Cancelled,
                plugin_id: plugin_id.clone(),
                command: command.clone(),
                message,
                // A cancelled call may succeed on retry — the caller
                // would have to choose not to cancel that one. Marked
                // retryable so retry policies don't treat cancellation
                // as a permanent failure of the underlying handler.
                retryable: true,
            },
        }
    }

    /// Like [`IpcErrorEnvelope::from_ipc_error`], but fills the empty
    /// `plugin_id` / `command` fields with the caller-supplied fallbacks.
    ///
    /// Used at the `kernel_invoke` boundary, where the bridge knows which
    /// plugin and command the caller addressed even when the kernel's
    /// `IpcError` variant doesn't carry that context (e.g.
    /// `SerializationFailed`, `DispatcherUnavailable`).
    #[must_use]
    pub fn from_ipc_error_in_context(
        err: &IpcError,
        fallback_plugin_id: &str,
        fallback_command: &str,
    ) -> Self {
        let mut env = Self::from_ipc_error(err);
        if env.plugin_id.is_empty() {
            env.plugin_id = fallback_plugin_id.to_string();
        }
        if env.command.is_empty() {
            env.command = fallback_command.to_string();
        }
        env
    }
}

/// Event bus errors visible to plugins.
#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/"))]
pub enum BusError {
    /// Reserved for a future event-bus implementation that surfaces
    /// shutdown explicitly to publishers. The current implementation
    /// (a `tokio::broadcast` channel) treats "no active subscribers"
    /// as a non-error condition for fan-out, so `publish_*` helpers
    /// in `nexus-kernel` discard the underlying `SendError` rather
    /// than wrapping it as `Closed`. Kept in the variant set so
    /// callers writing exhaustive `match` arms don't have to revisit
    /// when the implementation changes. See issue #81.
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
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/"))]
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

    // ── IpcErrorEnvelope mapping tests ──────────────────────────────────────

    #[test]
    fn envelope_maps_timeout() {
        let err = IpcError::Timeout {
            plugin_id: "com.test".to_string(),
            command: "ping".to_string(),
            timeout_ms: 1000,
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::Timeout);
        assert!(env.retryable, "Timeout must be retryable");
        assert_eq!(env.plugin_id, "com.test");
        assert_eq!(env.command, "ping");
        assert!(env.message.contains("1000"));
    }

    #[test]
    fn envelope_maps_plugin_not_found() {
        let err = IpcError::PluginNotFound {
            plugin_id: "com.test".to_string(),
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::DispatchFailed);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "com.test");
        assert_eq!(env.command, "");
    }

    #[test]
    fn envelope_maps_command_not_found() {
        let err = IpcError::CommandNotFound {
            plugin_id: "com.test".to_string(),
            command: "nope".to_string(),
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::DispatchFailed);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "com.test");
        assert_eq!(env.command, "nope");
    }

    #[test]
    fn envelope_maps_capability_denied() {
        let err = IpcError::CapabilityDenied {
            plugin_id: "com.caller".to_string(),
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::CapabilityDenied);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "com.caller");
        assert_eq!(env.command, "");
    }

    #[test]
    fn envelope_maps_plugin_crashed() {
        let err = IpcError::PluginCrashedDuringCall {
            plugin_id: "com.test".to_string(),
            command: "boom".to_string(),
            reason: String::new(),
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::PluginCrashed);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "com.test");
        assert_eq!(env.command, "boom");
    }

    #[test]
    fn envelope_maps_serialization_failed() {
        let err = IpcError::SerializationFailed {
            reason: "bad json".to_string(),
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::Serialization);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "");
        assert_eq!(env.command, "");
    }

    #[test]
    fn envelope_maps_deserialization_failed() {
        let err = IpcError::DeserializationFailed {
            reason: "not a u32".to_string(),
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::Serialization);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "");
        assert_eq!(env.command, "");
    }

    #[test]
    fn envelope_maps_dispatcher_unavailable() {
        let err = IpcError::DispatcherUnavailable;
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::DispatchFailed);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "");
        assert_eq!(env.command, "");
    }

    #[test]
    fn envelope_maps_reentrant_call() {
        let err = IpcError::ReentrantCall {
            plugin_id: "com.test".to_string(),
            command: "loop".to_string(),
        };
        let env = IpcErrorEnvelope::from_ipc_error(&err);
        assert_eq!(env.kind, IpcErrorKind::DispatchFailed);
        assert!(!env.retryable);
        assert_eq!(env.plugin_id, "com.test");
        assert_eq!(env.command, "loop");
    }

    #[test]
    fn envelope_in_context_fills_empty_fields() {
        let err = IpcError::SerializationFailed {
            reason: "bad json".to_string(),
        };
        let env = IpcErrorEnvelope::from_ipc_error_in_context(&err, "com.fallback", "do_thing");
        assert_eq!(env.kind, IpcErrorKind::Serialization);
        assert_eq!(env.plugin_id, "com.fallback");
        assert_eq!(env.command, "do_thing");
    }

    #[test]
    fn envelope_in_context_does_not_overwrite_present_fields() {
        let err = IpcError::Timeout {
            plugin_id: "com.real".to_string(),
            command: "ping".to_string(),
            timeout_ms: 100,
        };
        let env = IpcErrorEnvelope::from_ipc_error_in_context(&err, "com.fallback", "fallback_cmd");
        assert_eq!(env.plugin_id, "com.real");
        assert_eq!(env.command, "ping");
    }

    #[test]
    fn envelope_serializes_with_snake_case_keys() {
        let env = IpcErrorEnvelope {
            kind: IpcErrorKind::Timeout,
            plugin_id: "com.test".to_string(),
            command: "ping".to_string(),
            message: "timed out".to_string(),
            retryable: true,
        };
        let json = serde_json::to_string(&env).expect("serialize");
        assert!(json.contains("\"kind\":\"timeout\""), "got: {json}");
        assert!(json.contains("\"plugin_id\":\"com.test\""), "got: {json}");
        assert!(json.contains("\"retryable\":true"), "got: {json}");
        assert!(json.contains("\"command\":\"ping\""), "got: {json}");
        assert!(json.contains("\"message\":\"timed out\""), "got: {json}");
    }

    #[test]
    fn envelope_kind_serializes_each_variant_snake_case() {
        for (kind, expected) in [
            (IpcErrorKind::Timeout, "\"timeout\""),
            (IpcErrorKind::PluginCrashed, "\"plugin_crashed\""),
            (IpcErrorKind::CapabilityDenied, "\"capability_denied\""),
            (IpcErrorKind::DispatchFailed, "\"dispatch_failed\""),
            (IpcErrorKind::Serialization, "\"serialization\""),
            (IpcErrorKind::Unknown, "\"unknown\""),
        ] {
            let s = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(s, expected);
        }
    }
}
