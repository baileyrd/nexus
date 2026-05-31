//! Core plugin for the security subsystem.
//!
//! Registers as `com.nexus.security` and participates in the plugin lifecycle.
//! Publishes audit events (`com.nexus.security.audit.*`) to the kernel event
//! bus so other plugins and the TUI can subscribe to security-relevant activity.
//!
//! ADR-0009 hard-fail policy is enforced from `on_init`: a `CredentialVault`
//! probe runs before any subsystem starts, and a `KeyringUnavailable` error
//! aborts plugin init with the platform-specific remediation hint. The
//! `NEXUS_NO_KEYRING=1` escape hatch flows through `CredentialVault::available()`
//! which returns `Ok(())` in disabled mode — the rest of the system then
//! gets `KeyringDisabled` on individual credential operations rather than a
//! startup abort.

use std::collections::HashSet;
use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde_json::json;

use crate::{CredentialVault, SecurityError};

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.security";

/// IPC handler: read a secret by `(plugin_id, name)`.
pub const HANDLER_GET_SECRET: u32 = 1;
/// IPC handler: store a secret under `(plugin_id, name)`.
pub const HANDLER_SET_SECRET: u32 = 2;
/// IPC handler: remove a secret by `(plugin_id, name)`.
pub const HANDLER_DELETE_SECRET: u32 = 3;
/// IPC handler: list secret names for `plugin_id` (current session only).
pub const HANDLER_LIST_SECRET_NAMES: u32 = 4;
/// IPC handler: query the persisted audit log (BL-094).
/// Args: `{event_type?, plugin_id?, since_ts?, limit?}` → `Vec<AuditLogEntry>`.
pub const HANDLER_QUERY_AUDIT_LOG: u32 = 5;
/// IPC handler: prune persisted audit entries older than `before_ts` (BL-100).
/// Args: `{before_ts: i64}` → `{removed: u64}`.
pub const HANDLER_CLEAR_AUDIT_LOG: u32 = 6;
/// IPC handler: snapshot the kernel-metrics registry (BL-093). Args
/// are ignored; returns the full `MetricsSnapshot` JSON.
pub const HANDLER_METRICS_SNAPSHOT: u32 = 7;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::security::register`.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("get_secret", HANDLER_GET_SECRET),
    ("set_secret", HANDLER_SET_SECRET),
    ("delete_secret", HANDLER_DELETE_SECRET),
    ("list_secret_names", HANDLER_LIST_SECRET_NAMES),
    ("query_audit_log", HANDLER_QUERY_AUDIT_LOG),
    ("clear_audit_log", HANDLER_CLEAR_AUDIT_LOG),
    ("metrics_snapshot", HANDLER_METRICS_SNAPSHOT),
];

/// Type-erased probe used by `on_init` to decide whether the OS keyring is
/// reachable. The default impl calls `CredentialVault::new().available()`;
/// tests inject a stub via [`SecurityCorePlugin::with_probe`].
type KeyringProbe = Box<dyn Fn() -> Result<(), SecurityError> + Send + Sync>;

fn default_keyring_probe() -> KeyringProbe {
    Box::new(|| CredentialVault::new().available())
}

/// Core plugin for security integration.
///
/// # Lifecycle
///
/// | Hook | Action |
/// |------|--------|
/// | `on_init` | Probes the OS keyring (ADR-0009); returns `LifecycleError` if unavailable |
/// | `on_start` | Publishes `com.nexus.security.started` on the bus |
/// | `on_stop` | Publishes `com.nexus.security.stopped` on the bus |
pub struct SecurityCorePlugin {
    event_bus: Option<Arc<EventBus>>,
    keyring_probe: KeyringProbe,
    vault: CredentialVault,
    /// In-memory index of namespaced keys (`"{plugin_id}:{name}"`) set during
    /// the current session. Used by `list_secret_names` since the OS keyring
    /// does not support enumeration. Cleared on plugin restart — names from
    /// previous sessions are still retrievable by exact name but not listable.
    known_names: HashSet<String>,
}

impl SecurityCorePlugin {
    /// Create a new (unstarted) security plugin with the production keyring
    /// probe (`CredentialVault::new().available()`).
    #[must_use]
    pub fn new(event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            event_bus,
            keyring_probe: default_keyring_probe(),
            vault: CredentialVault::new(),
            known_names: HashSet::new(),
        }
    }

    /// Create a plugin with an injected keyring probe. Used by tests to
    /// pin the `KeyringUnavailable` / `KeyringDisabled` / `Ok` paths
    /// without relying on the host process's environment or D-Bus state.
    #[must_use]
    pub fn with_probe<F>(event_bus: Option<Arc<EventBus>>, probe: F) -> Self
    where
        F: Fn() -> Result<(), SecurityError> + Send + Sync + 'static,
    {
        Self {
            event_bus,
            keyring_probe: Box::new(probe),
            vault: CredentialVault::disabled(),
            known_names: HashSet::new(),
        }
    }

    /// Publish an audit event to the kernel bus (best-effort).
    ///
    /// Audit events use the `com.nexus.security.audit.*` namespace.
    pub fn publish_audit(&self, event_type: &str, payload: serde_json::Value) {
        if let Some(bus) = &self.event_bus {
            let type_id = format!("{PLUGIN_ID}.audit.{event_type}");
            if let Err(e) = bus.publish_plugin(PLUGIN_ID, &type_id, payload) {
                tracing::error!(
                    plugin_id = PLUGIN_ID,
                    event_type = %type_id,
                    error = %e,
                    "audit event dropped — bus publish failed"
                );
            }
        }
    }
}

impl CorePlugin for SecurityCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        // ADR-0009: refuse to start if the OS keyring is unreachable.
        // The escape hatch (`NEXUS_NO_KEYRING=1`) is honoured inside
        // `CredentialVault::available()`, which returns `Ok(())` in
        // disabled mode so the rest of the system boots and individual
        // credential operations fail loudly later.
        if let Err(e) = (self.keyring_probe)() {
            tracing::error!(
                plugin_id = PLUGIN_ID,
                error = %e,
                "keyring hard-fail (ADR-0009): refusing to start"
            );
            return Err(PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_init".to_string(),
                reason: e.to_string(),
            });
        }
        tracing::debug!(plugin_id = PLUGIN_ID, "security subsystem initialized");
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        if let Some(bus) = &self.event_bus {
            if let Err(e) = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.security.started",
                serde_json::json!({}),
            ) {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    error = %e,
                    "failed to publish security.started lifecycle event"
                );
            }
        }
        tracing::info!(plugin_id = PLUGIN_ID, "security subsystem started");
        Ok(())
    }

    fn on_stop(&mut self) {
        if let Some(bus) = &self.event_bus {
            if let Err(e) = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.security.stopped",
                serde_json::json!({}),
            ) {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    error = %e,
                    "failed to publish security.stopped lifecycle event"
                );
            }
        }
        tracing::info!(plugin_id = PLUGIN_ID, "security subsystem stopped");
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_GET_SECRET => {
                let key = vault_key(args)?;
                match self.vault.retrieve(&key) {
                    Ok(value) => Ok(json!({ "value": value })),
                    Err(SecurityError::CredentialNotFound(_))
                    | Err(SecurityError::KeyringDisabled) => Ok(json!({ "value": null })),
                    Err(e) => Err(map_err(e)),
                }
            }
            HANDLER_SET_SECRET => {
                let key = vault_key(args)?;
                let value = string_arg(args, "value")?;
                self.vault.store(&key, &value).map_err(map_err)?;
                self.known_names.insert(key);
                Ok(json!({ "ok": true }))
            }
            HANDLER_DELETE_SECRET => {
                let key = vault_key(args)?;
                match self.vault.delete(&key) {
                    Ok(()) | Err(SecurityError::CredentialNotFound(_)) => {
                        self.known_names.remove(&key);
                        Ok(json!({ "ok": true }))
                    }
                    Err(SecurityError::KeyringDisabled) => Ok(json!({ "ok": false })),
                    Err(e) => Err(map_err(e)),
                }
            }
            HANDLER_LIST_SECRET_NAMES => {
                let plugin_id = string_arg(args, "plugin_id")?;
                let prefix = format!("{plugin_id}:");
                let names: Vec<String> = self
                    .known_names
                    .iter()
                    .filter_map(|k| k.strip_prefix(&prefix).map(str::to_string))
                    .collect();
                Ok(json!({ "names": names }))
            }
            HANDLER_QUERY_AUDIT_LOG => {
                // Each filter is optional; missing → None → no constraint.
                let filter = nexus_kernel::audit_store::AuditQuery {
                    event_type: args
                        .get("event_type")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    plugin_id: args
                        .get("plugin_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    since_ts: args.get("since_ts").and_then(serde_json::Value::as_i64),
                    limit: args
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                };
                let entries = nexus_kernel::audit_store::query(&filter);
                Ok(serde_json::to_value(&entries).unwrap_or(json!([])))
            }
            HANDLER_METRICS_SNAPSHOT => {
                let snap =
                    nexus_kernel::metrics::global().map(nexus_kernel::KernelMetrics::snapshot);
                Ok(serde_json::to_value(&snap).unwrap_or(json!(null)))
            }
            HANDLER_CLEAR_AUDIT_LOG => {
                let before_ts = args
                    .get("before_ts")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: "missing 'before_ts' argument".to_string(),
                    })?;
                let removed = nexus_kernel::audit_store::clear(before_ts);
                Ok(json!({ "removed": removed }))
            }
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler id {handler_id}"),
            }),
        }
    }
}

/// Build the namespaced vault key `"{plugin_id}:{name}"` from IPC args.
fn vault_key(args: &serde_json::Value) -> Result<String, PluginError> {
    let plugin_id = string_arg(args, "plugin_id")?;
    let name = string_arg(args, "name")?;
    Ok(format!("{plugin_id}:{name}"))
}

/// Extract a required string argument by key.
fn string_arg(args: &serde_json::Value, key: &str) -> Result<String, PluginError> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("missing '{key}' argument"),
        })
}

/// Map a `SecurityError` to a `PluginError` for IPC return.
fn map_err(e: SecurityError) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Probe stub that always succeeds — substitutes for the real keyring
    /// probe so the existing lifecycle tests don't depend on the host
    /// machine's D-Bus / Keychain state.
    fn ok_probe() -> impl Fn() -> Result<(), SecurityError> + Send + Sync + 'static {
        || Ok(())
    }

    #[test]
    fn plugin_id_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.security");
    }

    #[test]
    fn on_init_succeeds_when_probe_ok() {
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        plugin.on_init().unwrap();
    }

    #[test]
    fn on_init_fails_loudly_when_keyring_unavailable() {
        // ADR-0009 / OI-21: a `KeyringUnavailable` from the probe must
        // surface as a `LifecycleError` so the kernel aborts plugin init
        // and the frontend stops boot. The remediation hint must reach
        // the user — propagate through the error message so the
        // platform-specific guidance from `CredentialVault::available()`
        // is visible at the failure site.
        let mut plugin = SecurityCorePlugin::with_probe(None, || {
            Err(SecurityError::KeyringUnavailable {
                reason: "D-Bus not running".to_string(),
                platform_hint: "Ensure gnome-keyring or KWallet is running.".to_string(),
            })
        });
        let err = plugin
            .on_init()
            .expect_err("on_init must propagate the probe failure");
        match err {
            PluginError::LifecycleError {
                plugin_id,
                hook,
                reason,
            } => {
                assert_eq!(plugin_id, PLUGIN_ID);
                assert_eq!(hook, "on_init");
                assert!(reason.contains("D-Bus not running"));
                assert!(reason.contains("gnome-keyring"));
            }
            other => panic!("expected LifecycleError, got {other:?}"),
        }
    }

    #[test]
    fn on_init_succeeds_when_probe_reports_disabled() {
        // `NEXUS_NO_KEYRING=1` causes `CredentialVault::available()` to
        // return `Ok(())` without touching the OS keyring. The plugin
        // should boot — individual credential ops will fail later with
        // `KeyringDisabled`, which is the documented escape-hatch
        // contract from ADR-0009.
        let mut plugin = SecurityCorePlugin::with_probe(None, || Ok(()));
        plugin.on_init().unwrap();
    }

    #[test]
    fn on_start_succeeds_without_bus() {
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        plugin.on_start().unwrap();
    }

    #[test]
    fn on_stop_succeeds_without_bus() {
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        plugin.on_stop();
    }

    #[test]
    fn dispatch_returns_error_for_unknown_handler() {
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        let result = plugin.dispatch(42, &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn dispatch_get_secret_returns_null_when_disabled() {
        // with_probe initialises a disabled vault — retrieve returns
        // KeyringDisabled which we map to {"value": null} so callers
        // can fall through to a default without special-casing the error.
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        let result = plugin
            .dispatch(
                HANDLER_GET_SECRET,
                &serde_json::json!({"plugin_id": "nexus.test", "name": "foo"}),
            )
            .unwrap();
        assert_eq!(result, serde_json::json!({"value": null}));
    }

    #[test]
    fn dispatch_set_secret_in_disabled_mode_errors() {
        // store() returns KeyringDisabled in disabled mode. Unlike
        // get/delete (which we soften to null/false), set surfaces the
        // error so the caller knows their secret was never persisted.
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        let err = plugin
            .dispatch(
                HANDLER_SET_SECRET,
                &serde_json::json!({
                    "plugin_id": "nexus.test",
                    "name": "foo",
                    "value": "bar",
                }),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { plugin_id, .. } => assert_eq!(plugin_id, PLUGIN_ID),
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_set_secret_missing_plugin_id_errors() {
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        let err = plugin
            .dispatch(
                HANDLER_SET_SECRET,
                &serde_json::json!({"name": "foo", "value": "bar"}),
            )
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("plugin_id"), "got: {reason}");
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_list_secret_names_filters_by_plugin_id() {
        // Pre-populate known_names directly (simulating prior set_secret
        // calls) — the keyring isn't consulted by list_secret_names since
        // the OS keyring doesn't support enumeration.
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        plugin.known_names.insert("nexus.foo:secret_a".to_string());
        plugin.known_names.insert("nexus.foo:secret_b".to_string());
        plugin.known_names.insert("nexus.bar:other".to_string());

        let result = plugin
            .dispatch(
                HANDLER_LIST_SECRET_NAMES,
                &serde_json::json!({"plugin_id": "nexus.foo"}),
            )
            .unwrap();
        let mut names: Vec<String> = result["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["secret_a".to_string(), "secret_b".to_string()]);
    }

    #[test]
    fn dispatch_delete_secret_in_disabled_mode_returns_ok_false() {
        // delete in disabled mode soft-fails so callers can run
        // best-effort cleanup without worrying about disabled keyrings.
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        let result = plugin
            .dispatch(
                HANDLER_DELETE_SECRET,
                &serde_json::json!({"plugin_id": "nexus.test", "name": "foo"}),
            )
            .unwrap();
        assert_eq!(result, serde_json::json!({"ok": false}));
    }

    #[test]
    fn on_start_publishes_event_to_bus() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            "com.nexus.security.".to_string(),
        ));

        let mut plugin = SecurityCorePlugin::with_probe(Some(Arc::clone(&bus)), ok_probe());
        plugin.on_start().unwrap();

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            nexus_kernel::NexusEvent::Custom { type_id, .. } => {
                assert_eq!(type_id, "com.nexus.security.started");
            }
            _ => panic!("expected Custom event"),
        }
    }
}
