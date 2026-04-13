//! Core plugin for the security subsystem.
//!
//! Registers as `com.nexus.security` and participates in the plugin lifecycle.
//! Publishes audit events (`com.nexus.security.audit.*`) to the kernel event
//! bus so other plugins and the TUI can subscribe to security-relevant activity.

use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.security";

/// Core plugin for security integration.
///
/// # Lifecycle
///
/// | Hook | Action |
/// |------|--------|
/// | `on_init` | Logs initialization; verifies event bus is available |
/// | `on_start` | Publishes `com.nexus.security.started` on the bus |
/// | `on_stop` | Publishes `com.nexus.security.stopped` on the bus |
pub struct SecurityCorePlugin {
    event_bus: Option<Arc<EventBus>>,
}

impl SecurityCorePlugin {
    /// Create a new (unstarted) security plugin.
    #[must_use]
    pub fn new(event_bus: Option<Arc<EventBus>>) -> Self {
        Self { event_bus }
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

    #[test]
    fn plugin_id_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.security");
    }

    #[test]
    fn on_init_succeeds() {
        let mut plugin = SecurityCorePlugin::new(None);
        plugin.on_init().unwrap();
    }

    #[test]
    fn on_start_succeeds_without_bus() {
        let mut plugin = SecurityCorePlugin::new(None);
        plugin.on_start().unwrap();
    }

    #[test]
    fn on_stop_succeeds_without_bus() {
        let mut plugin = SecurityCorePlugin::new(None);
        plugin.on_stop();
    }

    #[test]
    fn dispatch_returns_error_for_unknown_handler() {
        let mut plugin = SecurityCorePlugin::new(None);
        let result = plugin.dispatch(42, &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn on_start_publishes_event_to_bus() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            "com.nexus.security.".to_string(),
        ));

        let mut plugin = SecurityCorePlugin::new(Some(Arc::clone(&bus)));
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
