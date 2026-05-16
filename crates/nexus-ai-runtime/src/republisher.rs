//! BL-134 Phase 2b-ii — typed AiEvent republisher.
//!
//! Subscribes to `com.nexus.ai.stream_*` and
//! `com.nexus.agent.round_*` topics on the kernel bus, looks up
//! the runtime `task_id` for each event's `session_id` (correlation
//! map populated by [`crate::core_plugin::inject_session_id`] at
//! submit time), and republishes the inner payload as a typed
//! [`crate::events::AiEvent`] under
//! `com.nexus.ai.runtime.<variant>`.
//!
//! Events from sessions that weren't submitted through the runtime
//! (e.g. CLI-direct `nexus agent run`) are silently dropped — the
//! correlation lookup returns `None` and the subscriber moves on.
//! Translation logic is split into two pure helpers so the wire
//! semantics can be tested without spinning up the kernel bus.

use serde_json::Value;
use uuid::Uuid;

use crate::events::{topic_for, AiEvent};

/// Topic published by `com.nexus.ai::stream_chat` (and its `ask`
/// flavour) for each token chunk the provider returns. Used by the
/// republisher's subscribe filter and the translate helper.
pub(crate) const TOPIC_STREAM_CHUNK: &str = "com.nexus.ai.stream_chunk";

/// Topic published by `com.nexus.agent::session_run` whenever the
/// agent waits for round approval. Used by the republisher's
/// subscribe filter and the translate helper.
pub(crate) const TOPIC_ROUND_PROPOSED: &str = "com.nexus.agent.round_proposed";

/// Pure helper — extract the `session_id` field from a bus payload.
/// Returns `None` when the payload isn't an object, the field is
/// absent, or the value isn't a non-empty string.
///
/// Factored out so the subscriber's session-id lookup can be tested
/// against the actual payload shapes the agent and AI plugins emit
/// today (the shapes are at the IPC boundary; we shouldn't reach for
/// the producer-side types).
#[must_use]
pub fn extract_session_id(payload: &Value) -> Option<String> {
    let s = payload.get("session_id")?.as_str()?;
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Pure helper — translate a bus event (already correlated to a
/// runtime `task_id`) into the corresponding typed `AiEvent`.
/// Returns `None` for topics outside the runtime's translation set
/// (e.g. `stream_start` / `stream_done` — those don't have a typed
/// variant today since the underlying Finished/Failed event already
/// covers them).
///
/// Each translation is defensive: missing fields fall back to
/// sensible defaults (empty string, zero round, etc.) rather than
/// failing the whole translation — the goal is to surface
/// observability even when an upstream payload drifts.
#[must_use]
pub fn translate_bus_event(topic: &str, payload: &Value, task_id: Uuid) -> Option<AiEvent> {
    match topic {
        TOPIC_STREAM_CHUNK => {
            let text = payload
                .get("chunk")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(AiEvent::TokenChunk { task_id, text })
        }
        TOPIC_ROUND_PROPOSED => {
            // `round` is i64 in JSON; cap at u32. `text` is the
            // agent's narration for the round.
            let round = payload
                .get("round")
                .and_then(Value::as_i64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0);
            let narration = payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(AiEvent::RoundProposed {
                task_id,
                round,
                narration,
            })
        }
        _ => None,
    }
}

/// Build the topic suffix the republisher emits for a given inner
/// topic, or `None` if the inner topic isn't translated. Convenience
/// for tests / tracing.
#[must_use]
pub fn republish_topic(inner_topic: &str) -> Option<String> {
    let placeholder = match inner_topic {
        TOPIC_STREAM_CHUNK => AiEvent::TokenChunk {
            task_id: Uuid::nil(),
            text: String::new(),
        },
        TOPIC_ROUND_PROPOSED => AiEvent::RoundProposed {
            task_id: Uuid::nil(),
            round: 0,
            narration: String::new(),
        },
        _ => return None,
    };
    Some(topic_for(&placeholder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_session_id_handles_missing_and_empty() {
        assert_eq!(extract_session_id(&serde_json::json!({})), None);
        assert_eq!(extract_session_id(&serde_json::json!({"session_id": ""})), None);
        assert_eq!(extract_session_id(&serde_json::json!({"session_id": 42})), None);
        assert_eq!(
            extract_session_id(&serde_json::json!({"session_id": "abc"})),
            Some("abc".to_string())
        );
    }

    #[test]
    fn translate_stream_chunk_builds_typed_token_chunk() {
        let payload = serde_json::json!({
            "session_id": "ignored-here",
            "chunk": "hello world",
            "index": 7
        });
        let task_id = Uuid::new_v4();
        match translate_bus_event(TOPIC_STREAM_CHUNK, &payload, task_id) {
            Some(AiEvent::TokenChunk { task_id: t, text }) => {
                assert_eq!(t, task_id);
                assert_eq!(text, "hello world");
            }
            other => panic!("expected TokenChunk, got {other:?}"),
        }
    }

    #[test]
    fn translate_round_proposed_builds_typed_round_proposed() {
        let payload = serde_json::json!({
            "session_id": "s1",
            "round": 3,
            "text": "I need to write the file",
            "tool_calls": []
        });
        let task_id = Uuid::new_v4();
        match translate_bus_event(TOPIC_ROUND_PROPOSED, &payload, task_id) {
            Some(AiEvent::RoundProposed {
                task_id: t,
                round,
                narration,
            }) => {
                assert_eq!(t, task_id);
                assert_eq!(round, 3);
                assert_eq!(narration, "I need to write the file");
            }
            other => panic!("expected RoundProposed, got {other:?}"),
        }
    }

    #[test]
    fn translate_unknown_topic_returns_none() {
        let payload = serde_json::json!({ "session_id": "s1" });
        assert!(translate_bus_event("com.nexus.theme.reloaded", &payload, Uuid::new_v4()).is_none());
        // stream_start / stream_done aren't translated today — they
        // don't add information beyond the runtime's own Finished/
        // Failed envelope.
        assert!(
            translate_bus_event("com.nexus.ai.stream_start", &payload, Uuid::new_v4()).is_none()
        );
        assert!(
            translate_bus_event("com.nexus.ai.stream_done", &payload, Uuid::new_v4()).is_none()
        );
    }

    #[test]
    fn translate_defensively_handles_missing_fields() {
        // round=0 + empty narration when fields are absent — we want
        // observability even when an upstream payload drifts.
        let payload = serde_json::json!({ "session_id": "s1" });
        let task_id = Uuid::new_v4();
        match translate_bus_event(TOPIC_ROUND_PROPOSED, &payload, task_id) {
            Some(AiEvent::RoundProposed {
                round, narration, ..
            }) => {
                assert_eq!(round, 0);
                assert_eq!(narration, "");
            }
            _ => panic!("expected RoundProposed"),
        }
    }

    #[test]
    fn translate_round_caps_negative_round_to_zero() {
        // i64 < 0 → u32 try_from fails → default 0.
        let payload = serde_json::json!({
            "session_id": "s1",
            "round": -1,
            "text": "weird"
        });
        match translate_bus_event(TOPIC_ROUND_PROPOSED, &payload, Uuid::new_v4()) {
            Some(AiEvent::RoundProposed { round, .. }) => assert_eq!(round, 0),
            _ => panic!("expected RoundProposed"),
        }
    }

    #[test]
    fn republish_topic_round_trips_through_translate() {
        // The topic_for(AiEvent) machinery already keeps suffixes
        // stable; this test pins the inner-to-outer mapping the
        // subscriber relies on.
        assert_eq!(
            republish_topic(TOPIC_STREAM_CHUNK).as_deref(),
            Some("com.nexus.ai.runtime.token_chunk")
        );
        assert_eq!(
            republish_topic(TOPIC_ROUND_PROPOSED).as_deref(),
            Some("com.nexus.ai.runtime.round_proposed")
        );
        assert!(republish_topic("com.nexus.theme.reloaded").is_none());
    }
}
