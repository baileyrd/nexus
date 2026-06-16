//! Bus-event capture → episodic memory (AI-first-class: "everything on the bus").
//!
//! [`event_to_memory`] is the pure transform driving capture: it turns a
//! [`PublishedEvent`] into an episodic [`Memory`] — **except** events emitted by
//! the memory plugin itself, which are dropped to prevent a feedback loop (a
//! captured memory write itself emits bus events; re-capturing those would spin
//! forever). Secret-looking payload values are redacted before storage.
//!
//! The bus-subscription task that pumps this — `event_bus.subscribe(All)` →
//! `event_to_memory` → `MemoryDb::insert` — is wired bootstrap-side (the kernel
//! owns the `EventBus`). This module is the testable core of that pipeline.

use nexus_plugin_api::event::{NexusEvent, PublishedEvent};
use serde_json::Value;

use crate::model::{Memory, MemoryType};

/// The memory plugin's own reverse-DNS id. Events emitted by it (or in its
/// namespace) are never captured — that is the feedback-loop guard.
pub const SELF_PLUGIN_ID: &str = "com.nexus.memory";

/// Substrings (case-insensitive) that mark a JSON object key as carrying a
/// secret; matching values are replaced with `[redacted]` before storage.
const SECRET_KEY_MARKERS: &[&str] = &[
    "api_key",
    "apikey",
    "secret",
    "token",
    "password",
    "passwd",
    "authorization",
    "credential",
    "private_key",
];

/// Convert a bus event into an episodic [`Memory`], or `None` when it must not
/// be captured.
///
/// Returns `None` for events emitted by the memory plugin itself or within its
/// `com.nexus.memory.*` namespace (feedback-loop prevention). For everything
/// else, the event becomes an episodic memory tagged with its topic; the
/// (secret-redacted) payload and provenance live in `metadata`.
#[must_use]
pub fn event_to_memory(ev: &PublishedEvent) -> Option<Memory> {
    let source = ev.metadata.source_plugin_id.as_str();
    if source == SELF_PLUGIN_ID || emitted_in_self_namespace(&ev.event) {
        return None;
    }

    let (topic, payload, summary) = describe(&ev.event, source);
    let mut metadata = serde_json::json!({
        "event_id": ev.metadata.event_id,
        "topic": topic,
        "source": source,
        "timestamp": ev.metadata.timestamp,
    });
    if let Some(p) = payload {
        metadata["payload"] = redact(p);
    }

    let mut m = Memory::new(summary)
        .with_source("event")
        .with_type(MemoryType::Episodic)
        .with_category("event")
        .with_client(source.to_string())
        .with_tags([topic]);
    m.metadata = metadata;
    Some(m)
}

/// True when the event is a `Custom` signal in the memory plugin's namespace.
fn emitted_in_self_namespace(event: &NexusEvent) -> bool {
    matches!(
        event,
        NexusEvent::Custom { type_id, .. }
            if type_id == SELF_PLUGIN_ID
                || type_id.starts_with(&format!("{SELF_PLUGIN_ID}."))
    )
}

/// Derive `(topic, optional payload, human summary)` for an event.
fn describe(event: &NexusEvent, source: &str) -> (String, Option<Value>, String) {
    match event {
        NexusEvent::Custom { type_id, payload, .. } => {
            (type_id.clone(), Some(payload.clone()), type_id.clone())
        }
        NexusEvent::PluginLoaded { plugin_id, version } => (
            "PluginLoaded".to_string(),
            None,
            format!("plugin loaded: {plugin_id} v{version}"),
        ),
        NexusEvent::PluginStarted { plugin_id } => (
            "PluginStarted".to_string(),
            None,
            format!("plugin started: {plugin_id}"),
        ),
        NexusEvent::PluginStopped { plugin_id, reason } => (
            "PluginStopped".to_string(),
            None,
            format!("plugin stopped: {plugin_id} ({reason:?})"),
        ),
        NexusEvent::PluginCrashed { plugin_id, error } => (
            "PluginCrashed".to_string(),
            None,
            format!("plugin crashed: {plugin_id}: {error}"),
        ),
        NexusEvent::CapabilityGranted { plugin_id, .. } => (
            "CapabilityGranted".to_string(),
            None,
            format!("capability granted to {plugin_id} (from {source})"),
        ),
        NexusEvent::CapabilityDenied { plugin_id, .. } => (
            "CapabilityDenied".to_string(),
            None,
            format!("capability denied to {plugin_id} (from {source})"),
        ),
    }
}

/// Return `value` with secret-looking object values replaced by `[redacted]`.
fn redact(mut value: Value) -> Value {
    redact_in_place(&mut value);
    value
}

fn redact_in_place(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if is_secret_key(k) {
                    *v = Value::String("[redacted]".to_string());
                } else {
                    redact_in_place(v);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_in_place(item);
            }
        }
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SECRET_KEY_MARKERS.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_plugin_api::event::EventMetadata;

    fn meta(source: &str) -> EventMetadata {
        EventMetadata {
            event_id: uuid::Uuid::nil(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: source.to_string(),
            span_id: None,
        }
    }

    fn custom(source: &str, type_id: &str, payload: Value) -> PublishedEvent {
        PublishedEvent {
            metadata: meta(source),
            event: NexusEvent::Custom {
                type_id: type_id.to_string(),
                emitting_plugin: source.to_string(),
                payload,
            },
        }
    }

    #[test]
    fn captures_foreign_custom_event() {
        let ev = custom(
            "com.nexus.terminal",
            "com.nexus.terminal.command_run",
            serde_json::json!({ "cmd": "cargo test" }),
        );
        let m = event_to_memory(&ev).expect("should capture");
        assert_eq!(m.memory_type, MemoryType::Episodic);
        assert_eq!(m.source, "event");
        assert_eq!(m.category, "event");
        assert_eq!(m.client, "com.nexus.terminal");
        assert_eq!(m.tags, vec!["com.nexus.terminal.command_run".to_string()]);
        assert_eq!(m.metadata["payload"]["cmd"], "cargo test");
        assert_eq!(m.metadata["source"], "com.nexus.terminal");
    }

    #[test]
    fn drops_memory_plugins_own_events_by_source() {
        let ev = custom(
            SELF_PLUGIN_ID,
            "com.nexus.memory.added",
            serde_json::json!({ "id": "x" }),
        );
        assert!(event_to_memory(&ev).is_none(), "must not capture own events (loop guard)");
    }

    #[test]
    fn drops_memory_namespace_events_by_type_id() {
        // Even if mis-attributed source, a com.nexus.memory.* topic is dropped.
        let ev = custom(
            "com.nexus.kernel",
            "com.nexus.memory.added",
            serde_json::json!({}),
        );
        assert!(event_to_memory(&ev).is_none());
    }

    #[test]
    fn redacts_secret_payload_values() {
        let ev = custom(
            "com.nexus.ai",
            "com.nexus.ai.config_set",
            serde_json::json!({
                "provider": "openai",
                "api_key": "sk-secret-123",
                "nested": { "Authorization": "Bearer abc", "model": "gpt" }
            }),
        );
        let m = event_to_memory(&ev).unwrap();
        assert_eq!(m.metadata["payload"]["api_key"], "[redacted]");
        assert_eq!(m.metadata["payload"]["nested"]["Authorization"], "[redacted]");
        assert_eq!(m.metadata["payload"]["provider"], "openai");
        assert_eq!(m.metadata["payload"]["nested"]["model"], "gpt");
    }

    #[test]
    fn captures_kernel_lifecycle_event() {
        let ev = PublishedEvent {
            metadata: meta("kernel"),
            event: NexusEvent::PluginStarted {
                plugin_id: "com.nexus.git".to_string(),
            },
        };
        let m = event_to_memory(&ev).unwrap();
        assert_eq!(m.tags, vec!["PluginStarted".to_string()]);
        assert!(m.content.contains("com.nexus.git"));
    }
}
