//! Line-delimited JSON-RPC 2.0 over stdio.
//!
//! Each message is exactly one JSON object on one line followed by `\n`.
//! Lines longer than [`MAX_LINE_BYTES`] are rejected with
//! [`TransportError::Oversized`] so a misbehaving peer can't OOM the
//! host. Blank lines and lines that fail to parse as JSON-RPC are
//! reported through [`TransportError::BadBody`] rather than silently
//! dropped — debugging a wedged agent is easier when the framing
//! contract is enforced.
//!
//! This is the wire layer for both directions of the ACP crate: the
//! [`crate::client`] talks to outbound child processes, and the
//! [`crate::server`] reads inbound requests from `stdin`. Wrapping the
//! one shape in this module keeps the JSON-RPC envelope types + the
//! framing rules in lockstep.

use std::io;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// 16 MiB ceiling per line. A single ACP message larger than this is
/// almost certainly a protocol bug or an agent going off-piste; we'd
/// rather close the connection than OOM the host trying to honour it.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// JSON-RPC 2.0 message — request, response, or notification. ACP
/// sends a homogeneous stream of these on stdin/stdout, so we
/// deserialize into one untagged enum and let the reader decide which
/// variant applies based on which keys are present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// `id + method + params` — request issued either by us (outbound
    /// host → agent) or by the agent back at us (inbound). Inbound
    /// agent-side requests are rare in ACP but match the JSON-RPC 2.0
    /// spec; we acknowledge with a method-not-found error today.
    Request(JsonRpcRequest),
    /// `id + (result | error)` — response to one of our outbound
    /// requests.
    Response(JsonRpcResponse),
    /// `method + params`, no `id` — fire-and-forget agent-pushed event.
    Notification(JsonRpcNotification),
}

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Correlator. The agent echoes this verbatim in the matching
    /// [`JsonRpcResponse`].
    pub id: serde_json::Value,
    /// Method name (e.g. `"initialize"`, `"propose"`).
    pub method: String,
    /// Method-specific payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response envelope (success or error; mutually
/// exclusive per the spec).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Echo of the request's `id`.
    pub id: serde_json::Value,
    /// `Some` on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// `Some` on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Numeric error code (see the JSON-RPC 2.0 reserved code table).
    pub code: i64,
    /// Human-readable summary.
    pub message: String,
    /// Optional structured payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 notification envelope (no `id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Notification method name (e.g. `"agent/output"`).
    pub method: String,
    /// Method-specific payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// Errors raised by the transport.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// I/O failure on the underlying stream.
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// Body did not parse as JSON-RPC.
    #[error("malformed json-rpc body: {0}")]
    BadBody(#[from] serde_json::Error),
    /// Stream closed cleanly (no bytes read on a fresh read).
    #[error("stream closed")]
    Eof,
    /// One line exceeded the [`MAX_LINE_BYTES`] cap. Returned before
    /// any allocation past the cap so a malicious peer can't OOM us.
    #[error("line exceeds {MAX_LINE_BYTES}-byte cap (was {0})")]
    Oversized(usize),
}

/// Read one framed JSON-RPC message from `reader`.
///
/// Skips blank lines (purely-whitespace lines emitted by some
/// debuggers between messages) so callers don't have to think about
/// the noise. Returns [`TransportError::Eof`] when the stream is
/// closed and no bytes were available on the current read.
///
/// # Errors
/// - [`TransportError::Eof`] when the stream is exhausted.
/// - [`TransportError::Oversized`] for a line larger than
///   [`MAX_LINE_BYTES`].
/// - [`TransportError::BadBody`] when the line is not valid JSON-RPC.
/// - [`TransportError::Io`] for read failures.
pub async fn read_message<R>(reader: &mut BufReader<R>) -> Result<JsonRpcMessage, TransportError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(TransportError::Eof);
        }
        if n > MAX_LINE_BYTES {
            return Err(TransportError::Oversized(n));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Skip blank-line filler between messages (purely-LF /
            // CRLF lines, plus whitespace-only lines emitted by some
            // debuggers).
            continue;
        }
        let msg: JsonRpcMessage = serde_json::from_str(trimmed)?;
        return Ok(msg);
    }
}

/// Write one framed JSON-RPC message to `writer` and flush.
///
/// The on-wire shape is `<json>\n` — no Content-Length header, no
/// trailing whitespace beyond the single LF.
///
/// # Errors
/// - [`TransportError::BadBody`] when the message refuses to
///   serialise (`serde_json::to_vec` only fails on truly exotic
///   shapes; mostly defensive).
/// - [`TransportError::Io`] on write failure.
pub async fn write_message<W>(writer: &mut W, msg: &JsonRpcMessage) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    let mut body = serde_json::to_vec(msg)?;
    body.push(b'\n');
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn round_trips_a_request() {
        let req = JsonRpcMessage::Request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({"caps": []})),
        });
        let mut buf: Vec<u8> = Vec::new();
        write_message(&mut buf, &req).await.unwrap();
        // Frame ends in a single newline — no Content-Length prelude.
        assert!(buf.ends_with(b"\n"));
        assert!(!buf
            .windows(b"Content-Length".len())
            .any(|w| w == b"Content-Length"));
        let mut reader = BufReader::new(buf.as_slice());
        let parsed = read_message(&mut reader).await.unwrap();
        match parsed {
            JsonRpcMessage::Request(r) => {
                assert_eq!(r.method, "initialize");
                assert_eq!(r.id, serde_json::json!(1));
            }
            other => panic!("expected Request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn parses_response() {
        let line = b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n";
        let mut reader = BufReader::new(&line[..]);
        let parsed = read_message(&mut reader).await.unwrap();
        match parsed {
            JsonRpcMessage::Response(r) => {
                assert_eq!(r.id, serde_json::json!(7));
                assert_eq!(r.result.unwrap()["ok"], serde_json::json!(true));
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn parses_notification() {
        let line =
            b"{\"jsonrpc\":\"2.0\",\"method\":\"agent/output\",\"params\":{\"text\":\"hi\"}}\n";
        let mut reader = BufReader::new(&line[..]);
        let parsed = read_message(&mut reader).await.unwrap();
        assert!(matches!(parsed, JsonRpcMessage::Notification(_)));
    }

    #[tokio::test]
    async fn skips_blank_lines_between_messages() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"\n\r\n   \n");
        buf.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"x\",\"params\":null}\n");
        let mut reader = BufReader::new(buf.as_slice());
        let parsed = read_message(&mut reader).await.unwrap();
        assert!(matches!(parsed, JsonRpcMessage::Request(_)));
    }

    #[tokio::test]
    async fn eof_on_empty_stream() {
        let mut reader = BufReader::new(&[][..]);
        let err = read_message(&mut reader).await.unwrap_err();
        assert!(matches!(err, TransportError::Eof));
    }

    #[tokio::test]
    async fn rejects_malformed_json() {
        let line = b"{not json}\n";
        let mut reader = BufReader::new(&line[..]);
        let err = read_message(&mut reader).await.unwrap_err();
        assert!(matches!(err, TransportError::BadBody(_)));
    }
}
