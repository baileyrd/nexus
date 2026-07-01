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
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde_json::{json, Value};

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

/// `sandbox_policy` handler id — return the active OS-sandbox config.
pub const HANDLER_SANDBOX_POLICY: u32 = 8;

/// `download` handler id (async) — brokered, allowlisted download.
pub const HANDLER_DOWNLOAD: u32 = 9;

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
    ("sandbox_policy", HANDLER_SANDBOX_POLICY),
    ("download", HANDLER_DOWNLOAD),
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
    /// OS-sandbox configuration (`<forge>/.forge/sandbox.toml`); exposed via the
    /// `sandbox_policy` handler and consumed by the download broker.
    sandbox_config: crate::SandboxConfig,
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
            sandbox_config: crate::SandboxConfig::default(),
        }
    }

    /// Set the OS-sandbox configuration (loaded from `sandbox.toml` by
    /// bootstrap). Builder-style so existing call sites need no change.
    #[must_use]
    pub fn with_sandbox_config(mut self, config: crate::SandboxConfig) -> Self {
        self.sandbox_config = config;
        self
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
            sandbox_config: crate::SandboxConfig::default(),
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
                // #190 spirit — strict-parse via typed `GetSecretArgs`.
                let typed: crate::ipc::GetSecretArgs = parse_args(args, "get_secret")?;
                let key = format!("{}:{}", typed.plugin_id, typed.name);
                let value = match self.vault.retrieve(&key) {
                    Ok(v) => Some(v),
                    Err(SecurityError::CredentialNotFound(_) | SecurityError::KeyringDisabled) => {
                        None
                    }
                    Err(e) => return Err(map_err(e)),
                };
                to_typed(&crate::ipc::GetSecretResult { value }, "get_secret")
            }
            HANDLER_SET_SECRET => {
                let typed: crate::ipc::SetSecretArgs = parse_args(args, "set_secret")?;
                let key = format!("{}:{}", typed.plugin_id, typed.name);
                self.vault.store(&key, &typed.value).map_err(map_err)?;
                self.known_names.insert(key);
                to_typed(&crate::ipc::SetSecretResult { ok: true }, "set_secret")
            }
            HANDLER_DELETE_SECRET => {
                let typed: crate::ipc::DeleteSecretArgs = parse_args(args, "delete_secret")?;
                let key = format!("{}:{}", typed.plugin_id, typed.name);
                let ok = match self.vault.delete(&key) {
                    Ok(()) | Err(SecurityError::CredentialNotFound(_)) => {
                        self.known_names.remove(&key);
                        true
                    }
                    Err(SecurityError::KeyringDisabled) => false,
                    Err(e) => return Err(map_err(e)),
                };
                to_typed(&crate::ipc::DeleteSecretResult { ok }, "delete_secret")
            }
            HANDLER_LIST_SECRET_NAMES => {
                let typed: crate::ipc::ListSecretNamesArgs = parse_args(args, "list_secret_names")?;
                let prefix = format!("{}:", typed.plugin_id);
                let names: Vec<String> = self
                    .known_names
                    .iter()
                    .filter_map(|k| k.strip_prefix(&prefix).map(str::to_string))
                    .collect();
                to_typed(
                    &crate::ipc::ListSecretNamesResult { names },
                    "list_secret_names",
                )
            }
            HANDLER_QUERY_AUDIT_LOG => {
                // #190 spirit — strict-parse via typed `QueryAuditLogArgs`.
                let typed: crate::ipc::QueryAuditLogArgs = parse_args(args, "query_audit_log")?;
                let filter = nexus_kernel::audit_store::AuditQuery {
                    event_type: typed.event_type,
                    plugin_id: typed.plugin_id,
                    since_ts: typed.since_ts,
                    limit: typed.limit,
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
                let typed: crate::ipc::ClearAuditLogArgs = parse_args(args, "clear_audit_log")?;
                let removed = nexus_kernel::audit_store::clear(typed.before_ts);
                to_typed(
                    &crate::ipc::ClearAuditLogResult { removed },
                    "clear_audit_log",
                )
            }
            HANDLER_SANDBOX_POLICY => {
                // Read-only introspection of the active OS-sandbox config.
                Ok(serde_json::to_value(&self.sandbox_config).unwrap_or(json!(null)))
            }
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler id {handler_id}"),
            }),
        }
    }

    fn dispatch_async(&mut self, handler_id: u32, args: &Value) -> Option<CorePluginFuture> {
        // `download` performs outbound HTTP (async); everything else is sync.
        if handler_id != HANDLER_DOWNLOAD {
            return None;
        }
        let config = self.sandbox_config.clone();
        let args = args.clone();
        Some(Box::pin(async move {
            download_handler(config, args)
                .await
                .map_err(|reason| PluginError::ExecutionFailed {
                    plugin_id: PLUGIN_ID.to_string(),
                    reason,
                })
        }))
    }
}

/// Parse + validate a `download` request against the active [`SandboxConfig`]
/// (download allowlist + the policy's writable roots resolved against `cwd`,
/// which defaults to the destination's parent). Sync — does no I/O — so the
/// permission decision is unit-testable. Returns the validated URL, the
/// destination, and the size cap, ready for [`crate::downloads::fetch_url`].
fn prepare_download(
    config: &crate::SandboxConfig,
    args: &Value,
) -> Result<(reqwest::Url, std::path::PathBuf, u64), String> {
    let url = args
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "download: missing 'url'".to_string())?;
    let dest = args
        .get("dest")
        .and_then(Value::as_str)
        .ok_or_else(|| "download: missing 'dest'".to_string())?;
    let dest = std::path::PathBuf::from(dest);
    let cwd = args
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .or_else(|| dest.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default();

    let roots = config.policy.writable_roots_with_cwd(&cwd);
    let req = crate::DownloadRequest { url, dest: &dest };
    let validated =
        crate::downloads::validate(&req, &config.downloads, &roots).map_err(|e| e.to_string())?;
    Ok((validated, dest, config.downloads.max_bytes))
}

/// Async `download` handler: [`prepare_download`] (validate) then stream the
/// fetch. Returns `{ "bytes_written": N }`.
async fn download_handler(config: crate::SandboxConfig, args: Value) -> Result<Value, String> {
    let (url, dest, max_bytes) = prepare_download(&config, &args)?;
    let bytes = crate::downloads::fetch_url(url, &dest, max_bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "bytes_written": bytes }))
}

/// Strict-parse `args` into the typed envelope `T` (must be
/// `deny_unknown_fields`). Surfaces parse failures as
/// `PluginError::ExecutionFailed { reason: "<verb>: invalid args: …" }`.
fn parse_args<T>(args: &serde_json::Value, verb: &str) -> Result<T, PluginError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(args.clone()).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("{verb}: invalid args: {e}"),
    })
}

/// Serialise a typed reply to `serde_json::Value`, mapping
/// serialisation errors to `PluginError::ExecutionFailed`.
fn to_typed<T: serde::Serialize>(reply: &T, verb: &str) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(reply).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("{verb}: serialize reply: {e}"),
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
    fn prepare_download_enforces_policy() {
        use nexus_types::SandboxPolicy;

        // Default config: downloads disabled → refused before any I/O.
        let closed = crate::SandboxConfig::default();
        let args = json!({ "url": "https://h/x", "dest": "/work/x", "cwd": "/work" });
        assert!(prepare_download(&closed, &args)
            .unwrap_err()
            .contains("disabled"));

        // Enabled + allowlisted + workspace-write covering the dest → accepted.
        let open = crate::SandboxConfig {
            policy: SandboxPolicy::new_workspace_write(vec![std::path::PathBuf::from("/work")]),
            downloads: crate::DownloadPolicy {
                enabled: true,
                allowed_hosts: vec!["host.example".to_string()],
                max_bytes: 64,
            },
            ..Default::default()
        };
        let ok = json!({ "url": "https://host.example/a", "dest": "/work/a", "cwd": "/work" });
        let (url, dest, cap) = prepare_download(&open, &ok).unwrap();
        assert_eq!(url.host_str(), Some("host.example"));
        assert_eq!(dest, std::path::Path::new("/work/a"));
        assert_eq!(cap, 64);

        // Same config, dest outside the writable root → refused.
        let outside = json!({ "url": "https://host.example/a", "dest": "/etc/a", "cwd": "/work" });
        assert!(prepare_download(&open, &outside)
            .unwrap_err()
            .contains("not inside a writable root"));

        // Missing url → refused.
        assert!(prepare_download(&open, &json!({ "dest": "/work/a" }))
            .unwrap_err()
            .contains("missing 'url'"));
    }

    #[test]
    fn dispatch_sandbox_policy_returns_active_config() {
        // Default: closed (read-only, downloads off).
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        let out = plugin.dispatch(HANDLER_SANDBOX_POLICY, &json!({})).unwrap();
        assert_eq!(out["policy"]["mode"], "read-only");
        assert_eq!(out["downloads"]["enabled"], false);

        // Reflects an injected workspace-write config.
        let cfg = crate::SandboxConfig {
            policy: nexus_types::SandboxPolicy::new_workspace_write(vec![]),
            downloads: crate::DownloadPolicy {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe()).with_sandbox_config(cfg);
        let out = plugin.dispatch(HANDLER_SANDBOX_POLICY, &json!({})).unwrap();
        assert_eq!(out["policy"]["mode"], "workspace-write");
        assert_eq!(out["downloads"]["enabled"], true);
    }

    #[test]
    fn dispatch_sandbox_policy_exposes_bundled_shell_flag() {
        // M4: `bundled_shell_for_sandbox` is the config surface a confined-session
        // spawner (the agent) reads back over this introspection handler to set
        // `SessionConfig.bundled_shell`. Lock that it round-trips through the IPC
        // response so the knob is reachable, not silently dropped — off by
        // default, and reflected when enabled.
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe());
        let out = plugin.dispatch(HANDLER_SANDBOX_POLICY, &json!({})).unwrap();
        assert_eq!(out["bundled_shell_for_sandbox"], false);

        let cfg = crate::SandboxConfig {
            bundled_shell_for_sandbox: true,
            ..Default::default()
        };
        let mut plugin = SecurityCorePlugin::with_probe(None, ok_probe()).with_sandbox_config(cfg);
        let out = plugin.dispatch(HANDLER_SANDBOX_POLICY, &json!({})).unwrap();
        assert_eq!(out["bundled_shell_for_sandbox"], true);
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
