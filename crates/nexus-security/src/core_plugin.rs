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

use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};

use crate::{CredentialVault, SecurityError};

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.security";

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
}

impl SecurityCorePlugin {
    /// Create a new (unstarted) security plugin with the production keyring
    /// probe (`CredentialVault::new().available()`).
    #[must_use]
    pub fn new(event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            event_bus,
            keyring_probe: default_keyring_probe(),
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
        }
    }

    /// Publish an audit event to the kernel bus (best-effort).
    ///
    /// Audit events use the `com.nexus.security.audit.*` namespace.
    pub fn publish_audit(&self, event_type: &str, payload: serde_json::Value) {
        if let Some(bus) = &self.event_bus {
            let type_id = format!("{PLUGIN_ID}.audit.{event_type}");
            let _ = bus.publish_plugin(PLUGIN_ID, &type_id, payload);
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
        tracing::debug!(
            plugin_id = PLUGIN_ID,
            "security subsystem initialized"
        );
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.security.started",
                serde_json::json!({}),
            );
        }
        tracing::info!(plugin_id = PLUGIN_ID, "security subsystem started");
        Ok(())
    }

    fn on_stop(&mut self) {
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.security.stopped",
                serde_json::json!({}),
            );
        }
        tracing::info!(plugin_id = PLUGIN_ID, "security subsystem stopped");
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "unknown handler id {handler_id}; security IPC commands not yet registered"
            ),
        })
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
        let err = plugin.on_init().expect_err("on_init must propagate the probe failure");
        match err {
            PluginError::LifecycleError { plugin_id, hook, reason } => {
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
