//! Move 7 — every loop event as potential agent input.
//!
//! [`EventInput`] is the perception unit that represents a bus event
//! converted to agent-readable form.  [`AmbientTrigger`] binds an event
//! filter to a session template so that matching events automatically spawn
//! [`crate::session::SessionKind::SignalTriggered`] sessions, closing the
//! Perceive→Reason→Act→Observe loop.
//!
//! The [`TriggerRegistry`] is thread-safe and clone-able (shared `Arc`
//! backing). The trigger watcher task (started in
//! [`crate::core_plugin::AiRuntimeCorePlugin::wire_context`]) holds a clone
//! and reacts to every bus event in real time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use nexus_plugin_api::event::{NexusEvent, PublishedEvent};

// ─── TriggerId ───────────────────────────────────────────────────────────────

/// Unique identifier for an [`AmbientTrigger`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct TriggerId(pub Uuid);

impl TriggerId {
    /// Allocate a fresh random id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TriggerId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TriggerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ─── EventInputMode ──────────────────────────────────────────────────────────

/// How an event input should be consumed by the receiving session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum EventInputMode {
    /// Append the event as additional context to the current session round.
    Augment,
    /// Treat the event as a new top-level goal — replaces the current task
    /// description in the next spawned session.
    NewGoal,
    /// The event signals an interrupt — the receiving session should abort
    /// its current approach and replan.
    Interrupt,
}

// ─── EventInput ──────────────────────────────────────────────────────────────

/// A bus event converted into agent-perceptible input.
///
/// `EventInput` is the typed perception unit that crosses the bus-to-agent
/// boundary.  [`TriggerRegistry::matching`] drives how the watcher produces
/// these; the agent session receives one in its goal context when the trigger
/// fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EventInput {
    /// Unique id of the source [`PublishedEvent`] — used for dedup / tracing.
    pub event_id: Uuid,
    /// Namespaced event type: the variant name for kernel events
    /// (`"PluginLoaded"`, etc.) or the `type_id` string for `Custom` events.
    pub event_type: String,
    /// JSON payload extracted from the event.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub payload: serde_json::Value,
    /// Plugin that emitted the event (`"kernel"` for lifecycle events).
    pub source_plugin: String,
    /// UTC timestamp from the event metadata.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// How the receiving session should consume this input.
    pub mode: EventInputMode,
}

impl EventInput {
    /// Build an `EventInput` from a raw [`PublishedEvent`] and a consumption
    /// mode.
    #[must_use]
    pub fn from_published(event: &PublishedEvent, mode: EventInputMode) -> Self {
        let (event_type, payload) = extract_type_and_payload(&event.event);
        Self {
            event_id: event.metadata.event_id,
            event_type,
            payload,
            source_plugin: event.metadata.source_plugin_id.clone(),
            timestamp: event.metadata.timestamp,
            mode,
        }
    }
}

// ─── TriggerFilter ───────────────────────────────────────────────────────────

/// Serializable event-bus filter spec for an [`AmbientTrigger`]. Mirrors
/// [`nexus_plugin_api::event::EventFilter`] but derives `Serialize /
/// Deserialize` so it can be stored and transmitted over IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerFilter {
    /// Match every event on the bus. High-traffic — intended for broad ambient
    /// watchers; prefer a narrower filter for production triggers.
    All,
    /// Match a kernel-owned event variant by its name (e.g. `"PluginLoaded"`).
    KernelVariant {
        /// Variant name to match, e.g. `"PluginLoaded"`.
        name: String,
    },
    /// Match `Custom` events whose `type_id` starts with `prefix`.
    CustomPrefix {
        /// Type-id prefix to match against, e.g. `"com.nexus.storage."`.
        prefix: String,
    },
    /// Match exactly one `Custom` event `type_id`.
    CustomExact {
        /// Exact type-id to match, e.g. `"com.nexus.storage.file_changed"`.
        type_id: String,
    },
}

impl TriggerFilter {
    /// Returns `true` if `event` satisfies this filter.
    #[must_use]
    pub fn matches(&self, event: &NexusEvent) -> bool {
        match self {
            TriggerFilter::All => true,
            TriggerFilter::KernelVariant { name } => {
                kernel_variant_name(event) == Some(name.as_str())
            }
            TriggerFilter::CustomPrefix { prefix } => match event {
                NexusEvent::Custom { type_id, .. } => type_id.starts_with(prefix.as_str()),
                _ => false,
            },
            TriggerFilter::CustomExact { type_id: id } => match event {
                NexusEvent::Custom { type_id, .. } => type_id == id,
                _ => false,
            },
        }
    }

    /// Convert to the [`nexus_plugin_api::event::EventFilter`] variant that
    /// captures the same event set, for use when subscribing on the bus.
    #[must_use]
    pub fn to_event_filter(&self) -> nexus_plugin_api::event::EventFilter {
        use nexus_plugin_api::event::EventFilter;
        match self {
            TriggerFilter::All => EventFilter::All,
            TriggerFilter::KernelVariant { name } => EventFilter::Variant(name.clone()),
            TriggerFilter::CustomPrefix { prefix } => EventFilter::CustomPrefix(prefix.clone()),
            TriggerFilter::CustomExact { type_id } => EventFilter::CustomExact(type_id.clone()),
        }
    }
}

// ─── AmbientTrigger ──────────────────────────────────────────────────────────

/// Binds an event filter to a session template so that matching events
/// automatically spawn [`crate::session::SessionKind::SignalTriggered`]
/// sessions.
///
/// The trigger watcher loop (started in `wire_context`) polls all registered
/// triggers against every bus event and submits a new `SignalTriggered`
/// session for each match, with the rendered `goal_template` as the task
/// description.
///
/// # Goal template tokens
///
/// | Token | Replaced with |
/// |---|---|
/// | `{{event.type}}` | The event's type string |
/// | `{{event.payload}}` | The event's JSON payload (compact) |
/// | `{{event.source}}` | The emitting plugin id |
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct AmbientTrigger {
    /// Unique trigger id — assigned at construction, stable across
    /// enable/disable cycles.
    pub id: TriggerId,
    /// Human-readable name for observability panels.
    pub name: String,
    /// Which events to react to.
    pub filter: TriggerFilter,
    /// Goal string for the spawned session. Supports the interpolation
    /// tokens documented on the struct.
    pub goal_template: String,
    /// How the event should be presented to the spawned session.
    pub mode: EventInputMode,
    /// Disabled triggers are retained in the registry but silently
    /// ignored by the watcher loop.
    pub enabled: bool,
}

impl AmbientTrigger {
    /// Create a new trigger with `mode = NewGoal` and `enabled = true`.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        filter: TriggerFilter,
        goal_template: impl Into<String>,
    ) -> Self {
        Self {
            id: TriggerId::new(),
            name: name.into(),
            filter,
            goal_template: goal_template.into(),
            mode: EventInputMode::NewGoal,
            enabled: true,
        }
    }

    /// Override the mode. Builder-style.
    #[must_use]
    pub fn with_mode(mut self, mode: EventInputMode) -> Self {
        self.mode = mode;
        self
    }

    /// Render the goal string by substituting the `{{event.*}}` tokens with
    /// data from `published`.
    #[must_use]
    pub fn render_goal(&self, published: &PublishedEvent) -> String {
        let (event_type, payload) = extract_type_and_payload(&published.event);
        let payload_str = serde_json::to_string(&payload).unwrap_or_default();
        self.goal_template
            .replace("{{event.type}}", &event_type)
            .replace("{{event.payload}}", &payload_str)
            .replace("{{event.source}}", &published.metadata.source_plugin_id)
    }

    /// Returns `true` if this trigger is enabled and its filter matches
    /// `event`.
    #[must_use]
    pub fn matches(&self, event: &NexusEvent) -> bool {
        self.enabled && self.filter.matches(event)
    }
}

// ─── TriggerRegistry ─────────────────────────────────────────────────────────

/// Thread-safe, clone-able registry of [`AmbientTrigger`]s. Clone shares the
/// same backing `Arc` so all clones see the same trigger set.
#[derive(Debug, Default, Clone)]
pub struct TriggerRegistry {
    inner: Arc<Mutex<HashMap<TriggerId, AmbientTrigger>>>,
}

impl TriggerRegistry {
    /// #199 / R16 — recover from a poisoned lock rather than
    /// `.expect()`-panicking. With `panic = "abort"` in the release
    /// profile, that would convert a prior panic into a whole-process
    /// abort. Purely in-memory trigger registration for ambient-event
    /// matching (no disk/DB/IPC write while held); every mutation is a
    /// single bounded `insert`/`remove`. Worst case after recovery is
    /// a stale trigger list (one that should fire doesn't, or a
    /// removed one still matches once) — a functional miss, not
    /// corrupted state (see `docs/0.1.2/architecture.md` "Lock-poison
    /// policy").
    fn lock_inner(&self) -> std::sync::MutexGuard<'_, HashMap<TriggerId, AmbientTrigger>> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::error!("ai-runtime trigger registry mutex poisoned — recovering (see #199)");
                poisoned.into_inner()
            }
        }
    }

    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a trigger. Returns the trigger's id.
    pub fn register(&self, trigger: AmbientTrigger) -> TriggerId {
        let id = trigger.id.clone();
        self.lock_inner().insert(id.clone(), trigger);
        id
    }

    /// Remove a trigger by id. Returns `true` if it was present.
    pub fn unregister(&self, id: &TriggerId) -> bool {
        self.lock_inner().remove(id).is_some()
    }

    /// Return all enabled triggers whose filter matches `event`.
    #[must_use]
    pub fn matching(&self, event: &NexusEvent) -> Vec<AmbientTrigger> {
        let g = self.lock_inner();
        g.values().filter(|t| t.matches(event)).cloned().collect()
    }

    /// Return all triggers (enabled and disabled), sorted by name.
    #[must_use]
    pub fn list(&self) -> Vec<AmbientTrigger> {
        let g = self.lock_inner();
        let mut v: Vec<_> = g.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Number of registered triggers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock_inner().len()
    }

    /// `true` if no triggers are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Extract a stable type string and JSON payload from any [`NexusEvent`].
pub(crate) fn extract_type_and_payload(event: &NexusEvent) -> (String, serde_json::Value) {
    match event {
        NexusEvent::Custom {
            type_id, payload, ..
        } => (type_id.clone(), payload.clone()),
        NexusEvent::PluginLoaded { plugin_id, version } => (
            "PluginLoaded".into(),
            serde_json::json!({ "plugin_id": plugin_id, "version": version }),
        ),
        NexusEvent::PluginStarted { plugin_id } => (
            "PluginStarted".into(),
            serde_json::json!({ "plugin_id": plugin_id }),
        ),
        NexusEvent::PluginStopped { plugin_id, reason } => (
            "PluginStopped".into(),
            serde_json::json!({ "plugin_id": plugin_id, "reason": reason }),
        ),
        NexusEvent::PluginCrashed { plugin_id, error } => (
            "PluginCrashed".into(),
            serde_json::json!({ "plugin_id": plugin_id, "error": error }),
        ),
        NexusEvent::CapabilityGranted {
            plugin_id,
            capability,
        } => (
            "CapabilityGranted".into(),
            serde_json::json!({ "plugin_id": plugin_id, "capability": capability }),
        ),
        NexusEvent::CapabilityDenied {
            plugin_id,
            capability,
        } => (
            "CapabilityDenied".into(),
            serde_json::json!({ "plugin_id": plugin_id, "capability": capability }),
        ),
    }
}

fn kernel_variant_name(event: &NexusEvent) -> Option<&'static str> {
    match event {
        NexusEvent::PluginLoaded { .. } => Some("PluginLoaded"),
        NexusEvent::PluginStarted { .. } => Some("PluginStarted"),
        NexusEvent::PluginStopped { .. } => Some("PluginStopped"),
        NexusEvent::PluginCrashed { .. } => Some("PluginCrashed"),
        NexusEvent::CapabilityGranted { .. } => Some("CapabilityGranted"),
        NexusEvent::CapabilityDenied { .. } => Some("CapabilityDenied"),
        NexusEvent::Custom { .. } => None,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use nexus_plugin_api::event::{EventMetadata, NexusEvent, PublishedEvent};

    fn published(event: NexusEvent) -> PublishedEvent {
        PublishedEvent {
            metadata: EventMetadata {
                event_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                source_plugin_id: "com.test.plugin".into(),
                span_id: None,
            },
            event,
        }
    }

    fn custom_event(type_id: &str) -> NexusEvent {
        NexusEvent::Custom {
            type_id: type_id.into(),
            emitting_plugin: "com.test".into(),
            payload: serde_json::json!({"key": "value"}),
        }
    }

    #[test]
    fn trigger_filter_custom_prefix_matches_prefix() {
        let f = TriggerFilter::CustomPrefix {
            prefix: "com.nexus.storage.".into(),
        };
        assert!(f.matches(&custom_event("com.nexus.storage.file_changed")));
        assert!(!f.matches(&custom_event("com.nexus.ai.stream_chunk")));
    }

    #[test]
    fn trigger_filter_custom_exact_matches_only_exact() {
        let f = TriggerFilter::CustomExact {
            type_id: "com.nexus.storage.file_changed".into(),
        };
        assert!(f.matches(&custom_event("com.nexus.storage.file_changed")));
        assert!(!f.matches(&custom_event("com.nexus.storage.file_deleted")));
    }

    #[test]
    fn trigger_filter_kernel_variant_matches_variant() {
        let f = TriggerFilter::KernelVariant {
            name: "PluginLoaded".into(),
        };
        let event = NexusEvent::PluginLoaded {
            plugin_id: "foo".into(),
            version: "1.0".into(),
        };
        assert!(f.matches(&event));
        // Custom events don't match KernelVariant
        assert!(!f.matches(&custom_event("PluginLoaded")));
    }

    #[test]
    fn trigger_filter_all_matches_everything() {
        let f = TriggerFilter::All;
        assert!(f.matches(&NexusEvent::PluginStarted {
            plugin_id: "x".into()
        }));
        assert!(f.matches(&custom_event("anything")));
    }

    #[test]
    fn ambient_trigger_disabled_does_not_match() {
        let mut t = AmbientTrigger::new("t", TriggerFilter::All, "goal");
        t.enabled = false;
        assert!(!t.matches(&custom_event("anything")));
    }

    #[test]
    fn ambient_trigger_render_goal_interpolates_tokens() {
        let t = AmbientTrigger::new(
            "file-watch",
            TriggerFilter::All,
            "Handle {{event.type}} from {{event.source}}: {{event.payload}}",
        );
        let p = published(NexusEvent::Custom {
            type_id: "com.nexus.storage.file_changed".into(),
            emitting_plugin: "com.nexus.storage".into(),
            payload: serde_json::json!({"path": "/notes/foo.md"}),
        });
        let goal = t.render_goal(&p);
        assert!(goal.contains("com.nexus.storage.file_changed"));
        assert!(goal.contains("com.test.plugin")); // source_plugin_id from metadata
        assert!(goal.contains("/notes/foo.md"));
    }

    #[test]
    fn trigger_registry_register_and_list_sorted_by_name() {
        let reg = TriggerRegistry::new();
        assert!(reg.is_empty());
        reg.register(AmbientTrigger::new("z-trigger", TriggerFilter::All, "z"));
        reg.register(AmbientTrigger::new("a-trigger", TriggerFilter::All, "a"));
        let list = reg.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "a-trigger");
        assert_eq!(list[1].name, "z-trigger");
    }

    #[test]
    fn trigger_registry_unregister_returns_correct_bool() {
        let reg = TriggerRegistry::new();
        let t = AmbientTrigger::new("t", TriggerFilter::All, "x");
        let id = reg.register(t);
        assert!(reg.unregister(&id));
        assert!(!reg.unregister(&id)); // second removal
        assert!(reg.is_empty());
    }

    #[test]
    fn trigger_registry_matching_skips_disabled() {
        let reg = TriggerRegistry::new();
        let mut t = AmbientTrigger::new("t", TriggerFilter::All, "x");
        t.enabled = false;
        reg.register(t);
        assert!(reg.matching(&custom_event("any")).is_empty());
    }

    #[test]
    fn trigger_registry_clone_shares_state() {
        let reg = TriggerRegistry::new();
        let reg2 = reg.clone();
        reg.register(AmbientTrigger::new("t", TriggerFilter::All, "x"));
        assert_eq!(reg2.len(), 1); // clone sees the registration
    }

    #[test]
    fn event_input_from_published_extracts_fields() {
        let p = published(NexusEvent::Custom {
            type_id: "com.nexus.test.event".into(),
            emitting_plugin: "com.nexus.test".into(),
            payload: serde_json::json!(42),
        });
        let input = EventInput::from_published(&p, EventInputMode::Augment);
        assert_eq!(input.event_type, "com.nexus.test.event");
        assert_eq!(input.payload, serde_json::json!(42));
        assert_eq!(input.mode, EventInputMode::Augment);
        assert_eq!(input.event_id, p.metadata.event_id);
    }

    #[test]
    fn trigger_filter_to_event_filter_all() {
        use nexus_plugin_api::event::EventFilter;
        let f = TriggerFilter::All;
        assert!(matches!(f.to_event_filter(), EventFilter::All));
    }
}
