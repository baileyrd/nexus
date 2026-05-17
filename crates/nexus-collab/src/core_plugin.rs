//! `com.nexus.collab` core plugin — BL-143 Phase 2.2.
//!
//! One IPC handler: `publish_presence`. Frontends (CM6 cursor publisher
//! in the shell, eventual TUI status line, etc.) call it with a partial
//! [`PresenceCursor`]; the plugin stamps the configured peer
//! identity onto a [`PresenceEvent`] and publishes it on the kernel
//! event bus under [`PRESENCE_TOPIC`].
//!
//! The existing Phase 1.3 outbound subscription on the bus
//! (`COLLAB_TOPIC_PREFIX`) picks up the event and ships it through the
//! [`crate::ReconnectingClient`] to peers; no relay-side changes are
//! required.
//!
//! Why a separate handler vs. letting frontends publish directly:
//! the shell's [`nexus_kernel::context_impl`] publish path is
//! plugin-namespace-checked (`type_id` must start with the caller's
//! plugin id). Frontends don't own `com.nexus.collab`, so they cannot
//! publish on the presence topic from their own context. Centralising
//! through this handler also gives us one place to stamp the peer
//! identity from `[collab]` config, instead of trusting each frontend
//! to send the right `user_id`.

use std::sync::{Arc, RwLock};

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};

use crate::client::COLLAB_PLUGIN_ID;
use crate::presence::{PresenceCursor, PresenceEvent, PRESENCE_TOPIC};

/// Reverse-DNS identifier — matches [`COLLAB_PLUGIN_ID`] so the
/// outbound subscription in the bridge picks up presence events
/// authored here (per the Phase 1.3 anti-loop note in [`crate::client`],
/// events authored under `com.nexus.collab.bridge` are skipped on the
/// outbound side; events authored here under `com.nexus.collab` are
/// forwarded).
pub const PLUGIN_ID: &str = COLLAB_PLUGIN_ID;

/// Handler id for `publish_presence`.
pub const HANDLER_PUBLISH_PRESENCE: u32 = 1;

/// Local peer identity sourced from `[collab]` in `.forge/config.toml`.
/// The plugin holds this in an `RwLock` so the bootstrap can lazily
/// populate it; a `None` identity means collab is not configured for
/// this forge and [`HANDLER_PUBLISH_PRESENCE`] short-circuits with a
/// known `ExecutionFailed` so the shell can stop calling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPeer {
    pub user_id: String,
    pub display_name: String,
}

/// Args for `publish_presence`. The frontend never sends `user_id` or
/// `display_name` — those come from the plugin's [`LocalPeer`]. A
/// frontend that wants to surface "I cleared my cursor" sends
/// `cursor: None`; the plugin still stamps the identity so peers learn
/// "<name> is on the forge but not focused on a file".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PublishPresenceArgs {
    /// Optional cursor location. `None` is treated as "idle / no focus".
    #[serde(default)]
    pub cursor: Option<PresenceCursor>,
}

/// Reply for `publish_presence` — empty struct so the handler still
/// has a typed JSON return that can grow additively (e.g. a future
/// rate-limit signal).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PublishPresenceReply {}

/// Core plugin holding the local identity + a bus handle.
///
/// Constructed with `new(None, None)` when `[collab]` is absent —
/// `publish_presence` returns "not configured" and the shell suppresses
/// further calls. Constructed with `new(Some(bus), Some(identity))`
/// when the bootstrap reads a complete `[collab]` block.
pub struct CollabCorePlugin {
    bus: Option<Arc<EventBus>>,
    identity: Arc<RwLock<Option<LocalPeer>>>,
}

impl CollabCorePlugin {
    /// Build the plugin. Pass `None` for either argument to leave the
    /// handler in "not configured" mode; the lifecycle hooks still run.
    #[must_use]
    pub fn new(bus: Option<Arc<EventBus>>, identity: Option<LocalPeer>) -> Self {
        Self {
            bus,
            identity: Arc::new(RwLock::new(identity)),
        }
    }

    /// Replace the local identity at runtime. Reserved for a future
    /// settings-change hook; not wired into the public IPC surface
    /// today.
    pub fn set_identity(&self, identity: Option<LocalPeer>) {
        if let Ok(mut g) = self.identity.write() {
            *g = identity;
        }
    }

    fn snapshot_identity(&self) -> Option<LocalPeer> {
        self.identity.read().ok().and_then(|g| g.clone())
    }
}

impl CorePlugin for CollabCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_PUBLISH_PRESENCE => self.dispatch_publish_presence(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl CollabCorePlugin {
    fn dispatch_publish_presence(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: PublishPresenceArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("publish_presence: invalid args: {e}")))?;
        let Some(identity) = self.snapshot_identity() else {
            // Surface a stable message so the shell can detect this
            // condition without a wire-format match. Same shape as
            // every other "<crate>: <reason>" PluginError message.
            return Err(exec_err("publish_presence: collab not configured"));
        };
        let bus = self
            .bus
            .as_ref()
            .ok_or_else(|| exec_err("publish_presence: no event bus wired"))?;
        let event = PresenceEvent {
            user_id: identity.user_id,
            display_name: identity.display_name,
            cursor: a.cursor,
        };
        let payload = serde_json::to_value(&event)
            .map_err(|e| exec_err(format!("publish_presence: serialize: {e}")))?;
        bus.publish_plugin(PLUGIN_ID, PRESENCE_TOPIC, payload)
            .map_err(|e| exec_err(format!("publish_presence: bus publish: {e}")))?;
        serde_json::to_value(&PublishPresenceReply::default())
            .map_err(|e| exec_err(format!("publish_presence: serialize reply: {e}")))
    }
}

fn exec_err(msg: impl Into<String>) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: msg.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_kernel::{EventBus, EventFilter, EventSubscription, NexusEvent};

    fn identity() -> LocalPeer {
        LocalPeer {
            user_id: "alice".into(),
            display_name: "Alice".into(),
        }
    }

    /// Pull the next event off the subscription and decode the
    /// `NexusEvent::Custom` payload as a [`PresenceEvent`]. Panics if
    /// the next event isn't a `Custom` frame on the presence topic —
    /// these tests only publish through the handler, so any other
    /// frame is a regression.
    async fn next_presence(sub: &mut EventSubscription) -> PresenceEvent {
        let pe = sub.recv().await.expect("non-error event");
        let NexusEvent::Custom { type_id, payload, .. } = &pe.event else {
            panic!("expected Custom event, got {:?}", pe.event);
        };
        assert_eq!(type_id, PRESENCE_TOPIC);
        serde_json::from_value(payload.clone()).expect("payload decodes")
    }

    #[tokio::test]
    async fn publish_presence_publishes_event_on_presence_topic() {
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(PRESENCE_TOPIC.to_string()));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), Some(identity()));

        let v = plugin
            .dispatch(
                HANDLER_PUBLISH_PRESENCE,
                &serde_json::json!({
                    "cursor": {
                        "relpath": "notes/today.md",
                        "offset": 42,
                    },
                }),
            )
            .expect("handler runs");
        assert_eq!(v, serde_json::json!({}));

        let payload = next_presence(&mut sub).await;
        assert_eq!(payload.user_id, "alice");
        assert_eq!(payload.display_name, "Alice");
        assert_eq!(payload.cursor.as_ref().unwrap().relpath, "notes/today.md");
        assert_eq!(payload.cursor.as_ref().unwrap().offset, Some(42));
        assert_eq!(payload.cursor.as_ref().unwrap().selection_end, None);
    }

    #[tokio::test]
    async fn publish_presence_with_no_cursor_still_stamps_identity() {
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(PRESENCE_TOPIC.to_string()));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), Some(identity()));
        plugin
            .dispatch(HANDLER_PUBLISH_PRESENCE, &serde_json::json!({}))
            .expect("handler runs");
        let payload = next_presence(&mut sub).await;
        assert_eq!(payload.user_id, "alice");
        assert!(payload.cursor.is_none());
    }

    #[test]
    fn publish_presence_rejects_when_identity_missing() {
        let bus = Arc::new(EventBus::new(8));
        let mut plugin = CollabCorePlugin::new(Some(bus), None);
        let err = plugin
            .dispatch(HANDLER_PUBLISH_PRESENCE, &serde_json::json!({}))
            .unwrap_err();
        assert!(
            err.to_string().contains("collab not configured"),
            "stable error message: {err}"
        );
    }

    #[test]
    fn publish_presence_rejects_unknown_fields() {
        let bus = Arc::new(EventBus::new(8));
        let mut plugin = CollabCorePlugin::new(Some(bus), Some(identity()));
        let err = plugin
            .dispatch(
                HANDLER_PUBLISH_PRESENCE,
                &serde_json::json!({"cursor": null, "user_id": "trying to spoof"}),
            )
            .unwrap_err();
        assert!(err.to_string().contains("invalid args"), "{err}");
    }

    #[test]
    fn dispatch_unknown_handler_fails() {
        let mut plugin = CollabCorePlugin::new(None, None);
        let err = plugin
            .dispatch(99, &serde_json::json!({}))
            .unwrap_err();
        assert!(err.to_string().contains("unknown handler id 99"));
    }

    #[tokio::test]
    async fn set_identity_updates_in_place() {
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(PRESENCE_TOPIC.to_string()));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), None);
        plugin.set_identity(Some(LocalPeer {
            user_id: "bob".into(),
            display_name: "Bob".into(),
        }));
        plugin
            .dispatch(HANDLER_PUBLISH_PRESENCE, &serde_json::json!({}))
            .expect("handler runs after late identity wiring");
        let payload = next_presence(&mut sub).await;
        assert_eq!(payload.user_id, "bob");
    }
}
