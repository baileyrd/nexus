//! End-to-end test against a tiny mock LSP server (Python).
//!
//! The mock script speaks the LSP framing well enough to satisfy
//! `initialize` → `initialized`, echo back a hover response, and emit
//! a `publishDiagnostics` notification when a `didChange` arrives.
//! That's enough surface to exercise:
//!
//! - Process spawn / handshake
//! - Request → response correlation
//! - Server-pushed notification fan-out
//! - Graceful `shutdown` / `exit`
//!
//! If `python3` isn't on `$PATH` the test is silently skipped — that
//! keeps CI green on minimal-footprint runners that don't ship Python.

use std::path::PathBuf;
use std::time::Duration;

use nexus_lsp::{LspClient, LspServerSpec};
use serde_json::json;
use tokio::time::timeout;

/// Returns the path to a freshly written mock-server script, plus its
/// owning tempdir (callers must keep it alive for the test's
/// duration).
fn write_mock_server() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mock_lsp.py");
    let body = r#"#!/usr/bin/env python3
"""Tiny stdio LSP server used by nexus-lsp's end_to_end test.

Implements just enough of the protocol to:
- Acknowledge `initialize` with a minimal capabilities object.
- Acknowledge `initialized` (no reply, it's a notification).
- Reply to `textDocument/hover` with a fixed payload echoing the URI.
- On `textDocument/didChange`, emit a `publishDiagnostics` notification
  with one synthetic warning at line 0:0.
- Reply `null` to `shutdown`, then exit cleanly on `exit`.
"""
import json
import sys


def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.rstrip(b"\r\n")
        if not line:
            break
        key, _, value = line.partition(b":")
        headers[key.strip().lower()] = value.strip()
    length = int(headers[b"content-length"])
    body = sys.stdin.buffer.read(length)
    return json.loads(body)


def write_message(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


def main():
    while True:
        msg = read_message()
        if msg is None:
            return
        method = msg.get("method")
        if method == "initialize":
            write_message(
                {
                    "jsonrpc": "2.0",
                    "id": msg["id"],
                    "result": {
                        "capabilities": {
                            "textDocumentSync": 1,
                            "hoverProvider": True,
                        },
                        "serverInfo": {"name": "mock-lsp", "version": "0.0.1"},
                    },
                }
            )
        elif method == "initialized":
            pass
        elif method == "textDocument/hover":
            uri = msg.get("params", {}).get("textDocument", {}).get("uri", "?")
            write_message(
                {
                    "jsonrpc": "2.0",
                    "id": msg["id"],
                    "result": {
                        "contents": {"kind": "plaintext", "value": f"hover@{uri}"}
                    },
                }
            )
        elif method == "textDocument/didChange":
            uri = msg.get("params", {}).get("textDocument", {}).get("uri", "?")
            write_message(
                {
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": {
                        "uri": uri,
                        "diagnostics": [
                            {
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": 0, "character": 1},
                                },
                                "severity": 2,
                                "message": "synthetic warning",
                            }
                        ],
                    },
                }
            )
        elif method == "shutdown":
            write_message({"jsonrpc": "2.0", "id": msg["id"], "result": None})
        elif method == "exit":
            return
        elif msg.get("id") is not None:
            # Unknown request — return method-not-found
            write_message(
                {
                    "jsonrpc": "2.0",
                    "id": msg["id"],
                    "error": {"code": -32601, "message": f"unsupported {method}"},
                }
            )


if __name__ == "__main__":
    main()
"#;
    std::fs::write(&path, body).unwrap();
    // chmod +x so we can spawn directly via the absolute path on
    // platforms that look at the executable bit.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&path, perm).unwrap();
    }
    (dir, path)
}

fn python_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn handshake_request_and_diagnostic_round_trip() {
    if !python_available() {
        eprintln!("python3 not available — skipping");
        return;
    }
    let (_dir, script) = write_mock_server();
    let forge_dir = tempfile::tempdir().unwrap();
    let spec = LspServerSpec {
        name: "mock".to_string(),
        command: "python3".to_string(),
        args: vec![script.to_string_lossy().into_owned()],
        file_types: vec!["mock".to_string()],
        root_markers: vec![],
        disabled: false,
        env: Default::default(),
    };

    let client = timeout(
        Duration::from_secs(15),
        LspClient::connect("mock", &spec, forge_dir.path().to_path_buf()),
    )
    .await
    .expect("connect did not deadlock")
    .expect("handshake succeeded");

    // Hover request → response correlation.
    let hover = client
        .send_request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": "file:///x.mock" },
                "position": { "line": 0, "character": 0 },
            }),
        )
        .await
        .expect("hover succeeded");
    assert_eq!(
        hover["contents"]["value"].as_str(),
        Some("hover@file:///x.mock")
    );

    // didChange → publishDiagnostics fan-out.
    client
        .did_open("file:///x.mock", "mock", 1, "hello")
        .await
        .unwrap();
    client
        .did_change("file:///x.mock", 2, "hello world")
        .await
        .unwrap();
    let pushed = timeout(Duration::from_secs(5), client.next_notification())
        .await
        .expect("notification arrived")
        .expect("channel still open");
    assert_eq!(pushed.method, "textDocument/publishDiagnostics");
    assert_eq!(pushed.params["uri"].as_str(), Some("file:///x.mock"));
    assert_eq!(
        pushed.params["diagnostics"][0]["message"].as_str(),
        Some("synthetic warning")
    );

    // Graceful shutdown — drop the client; kill_on_drop reaps the child.
    drop(client);
}
