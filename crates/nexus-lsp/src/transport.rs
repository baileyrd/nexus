//! Framed JSON-RPC over stdio.
//!
//! LSP wraps JSON-RPC 2.0 in a Content-Length-prefixed envelope:
//!
//! ```text
//! Content-Length: 42\r\n
//! \r\n
//! {"jsonrpc":"2.0","id":1,"method":"…","params":{…}}
//! ```
//!
//! Optional `Content-Type` may appear; per the spec it MUST be the
//! UTF-8 JSON variant if present, so we accept and ignore it.
//!
//! This module is the wire layer only — request/response correlation,
//! initialize handshake, and document state live in [`crate::client`].

use std::io;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// JSON-RPC 2.0 message — request, response, or notification.
///
/// LSP sends a homogeneous stream of messages on the same channel, so
/// we deserialize into one tagged-by-presence-of-keys enum and let the
/// reader decide which variant applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// `id + method + params` — server-initiated request (rare in LSP)
    /// or client-initiated request being read back from a sub-process
    /// echo. We don't generate these; they appear when the server
    /// requests work from the client (e.g. `workspace/configuration`).
    Request(JsonRpcRequest),
    /// `id + (result | error)` — response to one of our outbound requests.
    Response(JsonRpcResponse),
    /// `method + params`, no `id` — fire-and-forget notification.
    Notification(JsonRpcNotification),
}

/// JSON-RPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Correlator returned in the matching [`JsonRpcResponse`].
    pub id: serde_json::Value,
    /// LSP method name, e.g. `"textDocument/completion"`.
    pub method: String,
    /// Method-specific JSON payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC response (success or error).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Echo of the [`JsonRpcRequest::id`] this response answers.
    pub id: serde_json::Value,
    /// `Some` on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// `Some` on failure. Mutually exclusive with `result` per JSON-RPC 2.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// LSP/JSON-RPC numeric error code.
    pub code: i64,
    /// Human-readable summary.
    pub message: String,
    /// Optional payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// JSON-RPC notification (no id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// LSP notification name, e.g. `"textDocument/publishDiagnostics"`.
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
    /// Header was missing or malformed (no `Content-Length`, garbage
    /// bytes, etc).
    #[error("malformed header: {0}")]
    BadHeader(String),
    /// Body did not parse as JSON-RPC.
    #[error("malformed json-rpc body: {0}")]
    BadBody(#[from] serde_json::Error),
    /// Stream closed mid-message.
    #[error("stream closed unexpectedly")]
    Eof,
}

/// Read one framed JSON-RPC message from `reader`.
///
/// # Errors
/// - [`TransportError::Eof`] when the stream returns 0 bytes before
///   any header bytes are seen — the canonical "child exited" path.
/// - [`TransportError::BadHeader`] for a malformed prelude.
/// - [`TransportError::BadBody`] when the body is not JSON-RPC.
/// - [`TransportError::Io`] for read failures.
pub async fn read_message<R>(reader: &mut BufReader<R>) -> Result<JsonRpcMessage, TransportError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    // 16 MiB ceiling — a single LSP message larger than this is almost
    // certainly a protocol bug or a server going off-piste, and we
    // don't want to OOM the host trying to honour it.
    const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
    // Header block: zero or more `Key: Value\r\n` lines, terminated by
    // an empty line (`\r\n`).
    let mut content_length: Option<usize> = None;
    let mut header_bytes = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            // EOF before any header — clean exit.
            if header_bytes == 0 {
                return Err(TransportError::Eof);
            }
            return Err(TransportError::BadHeader(
                "stream closed mid-header".to_string(),
            ));
        }
        header_bytes += n;
        // `read_line` keeps the `\n`. LSP uses `\r\n`; tolerate either.
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            return Err(TransportError::BadHeader(format!(
                "no colon in '{trimmed}'"
            )));
        };
        let key = key.trim();
        let value = value.trim();
        if key.eq_ignore_ascii_case("Content-Length") {
            content_length = Some(value.parse().map_err(|_| {
                TransportError::BadHeader(format!("non-numeric Content-Length '{value}'"))
            })?);
        }
        // Other headers (Content-Type) are accepted and ignored.
    }
    let Some(len) = content_length else {
        return Err(TransportError::BadHeader(
            "missing Content-Length".to_string(),
        ));
    };
    if len > MAX_BODY_BYTES {
        return Err(TransportError::BadHeader(format!(
            "Content-Length {len} exceeds {MAX_BODY_BYTES}-byte cap"
        )));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let msg: JsonRpcMessage = serde_json::from_slice(&buf)?;
    Ok(msg)
}

/// Write one framed JSON-RPC message to `writer` and flush.
///
/// # Errors
/// - [`TransportError::Io`] on write failure.
/// - [`TransportError::BadBody`] if the message can't be serialised
///   (a `serde_json::Value` containing a non-object root won't fail
///   here, so this branch is mostly defensive).
pub async fn write_message<W>(writer: &mut W, msg: &JsonRpcMessage) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(msg)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).await?;
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
            params: Some(serde_json::json!({"capabilities": {}})),
        });
        let mut buf: Vec<u8> = Vec::new();
        write_message(&mut buf, &req).await.unwrap();
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
        // 50 bytes — recompute on edit.
        let body = br#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#;
        let mut framed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        framed.extend_from_slice(body);
        let mut reader = BufReader::new(framed.as_slice());
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
    async fn parses_notification_with_extra_header() {
        let body = br#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{}}"#;
        let mut framed = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n\
             Content-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        framed.extend_from_slice(body);
        let mut reader = BufReader::new(framed.as_slice());
        let parsed = read_message(&mut reader).await.unwrap();
        assert!(matches!(parsed, JsonRpcMessage::Notification(_)));
    }

    #[tokio::test]
    async fn eof_before_any_byte_returns_eof() {
        let mut reader = BufReader::new(&[][..]);
        let err = read_message(&mut reader).await.unwrap_err();
        assert!(matches!(err, TransportError::Eof));
    }

    #[tokio::test]
    async fn missing_content_length_errors() {
        let buf = b"X-Foo: bar\r\n\r\n{}".to_vec();
        let mut reader = BufReader::new(buf.as_slice());
        let err = read_message(&mut reader).await.unwrap_err();
        assert!(matches!(err, TransportError::BadHeader(_)));
    }

    #[tokio::test]
    async fn oversized_message_rejected() {
        let buf = b"Content-Length: 99999999999\r\n\r\n".to_vec();
        let mut reader = BufReader::new(buf.as_slice());
        let err = read_message(&mut reader).await.unwrap_err();
        match err {
            TransportError::BadHeader(s) => assert!(s.contains("cap")),
            other => panic!("expected BadHeader, got {other:?}"),
        }
    }
}
