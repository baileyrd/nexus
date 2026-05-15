//! DAP wire envelope.
//!
//! Every DAP message carries a top-level `"type"` discriminator —
//! `"request"`, `"response"`, or `"event"`. Each variant has a stable
//! shape:
//!
//! ```json
//! { "seq": 1, "type": "request",  "command": "launch", "arguments": {…} }
//! { "seq": 2, "type": "response", "request_seq": 1, "success": true,
//!   "command": "launch", "body": {…} }
//! { "seq": 3, "type": "event",    "event": "stopped", "body": {…} }
//! ```
//!
//! `seq` is a monotonic integer per direction (client and adapter
//! each maintain their own counter). Responses correlate to requests
//! via `request_seq`. Errors travel as `success: false` with an
//! optional `message` and `body`.

use serde::{Deserialize, Serialize};

/// One message on the DAP wire.
///
/// The `serde(tag = "type")` adjacency lets us round-trip every
/// variant through the on-wire JSON shape without an extra wrapper
/// object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProtocolMessage {
    /// Client → adapter request, or adapter → client (rare — DAP
    /// allows server-initiated requests, e.g. `runInTerminal`).
    Request(ProtocolRequest),
    /// Response correlating to a previously sent request.
    Response(ProtocolResponse),
    /// Asynchronous notification from the adapter (stopped, output,
    /// terminated, …).
    Event(ProtocolEvent),
}

/// DAP request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolRequest {
    /// Monotonic per-direction sequence id.
    pub seq: i64,
    /// Command name, e.g. `"launch"`, `"setBreakpoints"`,
    /// `"stackTrace"`.
    pub command: String,
    /// Command-specific JSON payload. Absent for argless commands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// DAP response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolResponse {
    /// Monotonic per-direction sequence id.
    pub seq: i64,
    /// Echo of the originating [`ProtocolRequest::seq`].
    pub request_seq: i64,
    /// `true` on success; `false` indicates the adapter could not
    /// satisfy the command — `message` and `body` may carry detail.
    pub success: bool,
    /// Command name echoed verbatim. Mirrors the originating request.
    pub command: String,
    /// Human-readable error summary when `success == false`. Spec also
    /// allows `"cancelled"` and `"notStopped"` as well-known error
    /// shapes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Command-specific reply payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// DAP event body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolEvent {
    /// Monotonic per-direction sequence id.
    pub seq: i64,
    /// Event name (`"stopped"`, `"output"`, `"terminated"`, …).
    pub event: String,
    /// Event-specific payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_through_serde() {
        let req = ProtocolMessage::Request(ProtocolRequest {
            seq: 1,
            command: "launch".to_string(),
            arguments: Some(serde_json::json!({"program": "/bin/true"})),
        });
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains(r#""type":"request""#));
        assert!(s.contains(r#""command":"launch""#));
        let back: ProtocolMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, ProtocolMessage::Request(_)));
    }

    #[test]
    fn response_round_trips_with_error_message() {
        let resp = ProtocolMessage::Response(ProtocolResponse {
            seq: 4,
            request_seq: 3,
            success: false,
            command: "launch".to_string(),
            message: Some("program not found".to_string()),
            body: None,
        });
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains(r#""success":false"#));
        assert!(s.contains(r#""message":"program not found""#));
        // `body: None` must be omitted, not serialised as `null`.
        assert!(!s.contains(r#""body":null"#));
    }

    #[test]
    fn event_round_trips_with_body() {
        let evt = ProtocolMessage::Event(ProtocolEvent {
            seq: 9,
            event: "stopped".to_string(),
            body: Some(serde_json::json!({"reason": "breakpoint", "threadId": 1})),
        });
        let s = serde_json::to_string(&evt).unwrap();
        let back: ProtocolMessage = serde_json::from_str(&s).unwrap();
        let ProtocolMessage::Event(e) = back else {
            panic!("expected Event")
        };
        assert_eq!(e.event, "stopped");
        assert_eq!(e.body.unwrap()["threadId"], serde_json::json!(1));
    }

    #[test]
    fn unknown_type_field_fails_parse() {
        // serde(tag = ...) rejects any variant we didn't enumerate —
        // protects against silent data loss when adapters extend the
        // protocol.
        let s = r#"{"seq":1,"type":"reverseRequest","command":"runInTerminal"}"#;
        let r: Result<ProtocolMessage, _> = serde_json::from_str(s);
        assert!(r.is_err());
    }
}
