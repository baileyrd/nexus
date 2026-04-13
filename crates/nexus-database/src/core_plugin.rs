//! Core plugin for the database engine.
//!
//! Registers as `com.nexus.database` and publishes database events
//! (record CRUD, schema changes) to the kernel event bus.

use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.database";

/// Core plugin for the database engine.
///
/// Publishes `com.nexus.database.*` events on the kernel event bus
/// so other plugins can react to database changes.
pub struct DatabaseCorePlugin {
    event_bus: Arc<EventBus>,
}

impl DatabaseCorePlugin {
    /// Create a new database core plugin.
    #[must_use]
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self { event_bus }
    }

    /// Publish a database event to the kernel bus (best-effort).
    pub fn publish_event(&self, event_type: &str, payload: serde_json::Value) {
        let type_id = format!("{PLUGIN_ID}.{event_type}");
        let _ = self.event_bus.publish_plugin(PLUGIN_ID, &type_id, payload);
    }
}

impl CorePlugin for DatabaseCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        tracing::debug!(plugin_id = PLUGIN_ID, "database engine initialized");
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        self.publish_event("started", serde_json::json!({}));
        tracing::info!(plugin_id = PLUGIN_ID, "database engine started");
        Ok(())
    }

    fn on_stop(&mut self) {
        self.publish_event("stopped", serde_json::json!({}));
        tracing::info!(plugin_id = PLUGIN_ID, "database engine stopped");
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "unknown handler id {handler_id}; database IPC commands not yet registered"
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.database");
    }

    #[test]
    fn lifecycle_succeeds() {
        let bus = Arc::new(EventBus::new(16));
        let mut plugin = DatabaseCorePlugin::new(Arc::clone(&bus));
        plugin.on_init().unwrap();
        plugin.on_start().unwrap();
        plugin.on_stop();
    }

    #[test]
    fn publishes_started_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            "com.nexus.database.".to_string(),
        ));

        let mut plugin = DatabaseCorePlugin::new(Arc::clone(&bus));
        plugin.on_start().unwrap();

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            nexus_kernel::NexusEvent::Custom { type_id, .. } => {
                assert_eq!(type_id, "com.nexus.database.started");
            }
            _ => panic!("expected Custom event"),
        }
    }

    #[test]
    fn dispatch_unknown_handler_errors() {
        let bus = Arc::new(EventBus::new(16));
        let mut plugin = DatabaseCorePlugin::new(bus);
        assert!(plugin.dispatch(99, &serde_json::json!({})).is_err());
    }
}
