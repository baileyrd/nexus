//! AIG-07 — pure parsing helpers for `com.nexus.ai.stream_*` bus
//! events.
//!
//! The TUI subscribes to the chunk topic before firing
//! `stream_chat` / `stream_ask`, then drains the subscription
//! synchronously between render frames in `pump_ai`. These helpers
//! sit between the raw `serde_json::Value` payload and the panel
//! state mutation so the matching logic is unit-testable without
//! standing up a kernel context.
//!
//! Topic conventions (mirrored from `crates/nexus-ai/src/core_plugin.rs`):
//!
//! - `com.nexus.ai.stream_start`  → `{ session_id, sources? }`
//! - `com.nexus.ai.stream_chunk`  → `{ session_id, chunk, index }`
//! - `com.nexus.ai.stream_done`   → `{ session_id, text }`
//!
//! Multiple concurrent stream sessions can publish on the same
//! topics, so every helper here filters by `session_id` against
//! the caller's expected id and returns `None` for anything else.

use serde_json::Value;

/// Topic names — kept as constants so the kernel-side strings can't
/// drift from the TUI-side filter without a compile-time test pinning
/// both.
pub const STREAM_START_TOPIC: &str = "com.nexus.ai.stream_start";
pub const STREAM_CHUNK_TOPIC: &str = "com.nexus.ai.stream_chunk";
pub const STREAM_DONE_TOPIC: &str = "com.nexus.ai.stream_done";

/// If `payload` is a `stream_chunk` event whose `session_id` matches
/// `expected`, return the chunk text. `None` for any mismatch
/// (different session, missing field, wrong type).
#[must_use]
pub fn parse_chunk_event(payload: &Value, expected: &str) -> Option<String> {
    let sid = payload.get("session_id")?.as_str()?;
    if sid != expected {
        return None;
    }
    payload.get("chunk")?.as_str().map(str::to_string)
}

/// If `payload` is a `stream_done` event whose `session_id` matches
/// `expected`, return the final assembled text. The TUI uses this to
/// reconcile the streamed chunks against the post-processed final
/// (the kernel may apply `trim` / `stop` after the chunks streamed).
#[must_use]
pub fn parse_done_event(payload: &Value, expected: &str) -> Option<String> {
    let sid = payload.get("session_id")?.as_str()?;
    if sid != expected {
        return None;
    }
    payload.get("text")?.as_str().map(str::to_string)
}

/// True if `payload` is a `stream_start` event for `expected`. Used
/// purely to flip the panel indicator from "thinking…" to
/// "streaming…" once the kernel acknowledges the run; we don't read
/// the `sources` field on this side.
#[must_use]
pub fn matches_start_event(payload: &Value, expected: &str) -> bool {
    payload
        .get("session_id")
        .and_then(Value::as_str)
        .is_some_and(|sid| sid == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_chunk_event_returns_chunk_when_session_matches() {
        let payload = json!({
            "session_id": "abc",
            "chunk": "hello",
            "index": 0,
        });
        assert_eq!(parse_chunk_event(&payload, "abc"), Some("hello".into()));
    }

    #[test]
    fn parse_chunk_event_filters_out_mismatched_session() {
        let payload = json!({
            "session_id": "other",
            "chunk": "hello",
            "index": 0,
        });
        // The TUI should never confuse one session's stream for
        // another — concurrent panels (or future split-pane layouts)
        // might subscribe at the same time.
        assert_eq!(parse_chunk_event(&payload, "abc"), None);
    }

    #[test]
    fn parse_chunk_event_returns_none_on_missing_session_id() {
        let payload = json!({ "chunk": "hello", "index": 0 });
        assert_eq!(parse_chunk_event(&payload, "abc"), None);
    }

    #[test]
    fn parse_chunk_event_returns_none_on_missing_chunk() {
        let payload = json!({ "session_id": "abc", "index": 0 });
        assert_eq!(parse_chunk_event(&payload, "abc"), None);
    }

    #[test]
    fn parse_chunk_event_returns_none_on_non_string_chunk() {
        let payload = json!({ "session_id": "abc", "chunk": 42, "index": 0 });
        assert_eq!(parse_chunk_event(&payload, "abc"), None);
    }

    #[test]
    fn parse_chunk_event_passes_through_empty_chunk() {
        // The kernel's chunk_sink fires once per provider chunk.
        // An empty string is a legal payload (some providers emit
        // them as keepalives). The TUI should append it (no-op on
        // visible state) rather than filtering — filtering would
        // mis-count the sequence.
        let payload = json!({ "session_id": "abc", "chunk": "", "index": 5 });
        assert_eq!(parse_chunk_event(&payload, "abc"), Some(String::new()));
    }

    #[test]
    fn parse_done_event_returns_final_text_when_session_matches() {
        let payload = json!({ "session_id": "abc", "text": "all done" });
        assert_eq!(parse_done_event(&payload, "abc"), Some("all done".into()));
    }

    #[test]
    fn parse_done_event_filters_out_mismatched_session() {
        let payload = json!({ "session_id": "other", "text": "done" });
        assert_eq!(parse_done_event(&payload, "abc"), None);
    }

    #[test]
    fn parse_done_event_returns_none_on_missing_text() {
        let payload = json!({ "session_id": "abc" });
        assert_eq!(parse_done_event(&payload, "abc"), None);
    }

    #[test]
    fn matches_start_event_true_when_session_matches() {
        let payload = json!({ "session_id": "abc", "sources": [] });
        assert!(matches_start_event(&payload, "abc"));
    }

    #[test]
    fn matches_start_event_false_for_mismatched_session() {
        let payload = json!({ "session_id": "other" });
        assert!(!matches_start_event(&payload, "abc"));
    }

    #[test]
    fn topic_constants_match_kernel_names() {
        // Pin the topic strings — drift here would silently
        // disconnect the TUI from the kernel's stream surface.
        // Source: crates/nexus-ai/src/core_plugin.rs.
        assert_eq!(STREAM_START_TOPIC, "com.nexus.ai.stream_start");
        assert_eq!(STREAM_CHUNK_TOPIC, "com.nexus.ai.stream_chunk");
        assert_eq!(STREAM_DONE_TOPIC, "com.nexus.ai.stream_done");
    }
}
