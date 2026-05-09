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

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use nexus_lsp::pool::{ConnectionPool, PoolConfig};
use nexus_lsp::{LspClient, LspClientError, LspHostConfig, LspServerSpec};
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

#[tokio::test]
async fn documents_snapshot_returns_open_set() {
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
    .unwrap()
    .unwrap();

    // Empty before any did_open.
    assert!(client.documents_snapshot().await.is_empty());

    client
        .did_open("file:///a.mock", "mock", 1, "hello")
        .await
        .unwrap();
    client
        .did_open("file:///b.mock", "mock", 1, "world")
        .await
        .unwrap();

    let snap = client.documents_snapshot().await;
    assert_eq!(snap.len(), 2);
    let mut by_uri: HashMap<String, _> = HashMap::new();
    for d in snap {
        by_uri.insert(d.uri.clone(), d);
    }
    assert_eq!(by_uri["file:///a.mock"].text, "hello");
    assert_eq!(by_uri["file:///b.mock"].text, "world");
    assert_eq!(by_uri["file:///a.mock"].language_id, "mock");

    // did_close removes the entry from the snapshot set.
    client.did_close("file:///a.mock").await.unwrap();
    let snap = client.documents_snapshot().await;
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].uri, "file:///b.mock");

    drop(client);
}

#[tokio::test]
async fn call_with_reconnect_replays_open_documents_after_transient_failure() {
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

    // Stand up a host config the pool can route by name.
    let mut servers = HashMap::new();
    servers.insert("mock".to_string(), spec.clone());
    let cfg = LspHostConfig { servers };

    // Tight backoff so the test stays under a second.
    let pool_cfg = PoolConfig {
        backoff: vec![std::time::Duration::from_millis(50)],
    };
    let pool = ConnectionPool::new(pool_cfg, forge_dir.path().to_path_buf());

    // Connect once and seed a tracked open document on the original
    // client. This simulates the user having opened a tab before
    // the server crashes.
    {
        let client = pool.get_or_connect("mock", &cfg).await.unwrap();
        let lock = client.lock().await;
        lock.did_open("file:///x.mock", "mock", 1, "hello")
            .await
            .unwrap();
        // Sanity: the seeded open doc shows up in the snapshot.
        assert_eq!(lock.documents_snapshot().await.len(), 1);
    }

    // Drive `call_with_reconnect` against an op closure that fails
    // transiently on the first attempt and observes the document
    // set on the second attempt — that second attempt is running
    // against a *fresh* connection (the failure dropped the
    // entry), so the only way docs_snapshot sees the open doc is
    // if the pool replayed it during reconnect.
    let attempt = Arc::new(AtomicUsize::new(0));
    let resync_count = Arc::new(AtomicUsize::new(0));
    let attempt_for_op = Arc::clone(&attempt);
    let resync_for_op = Arc::clone(&resync_count);
    let _ = pool
        .call_with_reconnect("mock", &cfg, move |client| {
            let attempt = Arc::clone(&attempt_for_op);
            let resync = Arc::clone(&resync_for_op);
            Box::pin(async move {
                let n = attempt.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    // Force a transient classification — RequestTimeout
                    // is in `is_transient`'s match list, so the pool
                    // will drop the entry, snapshot docs, and replay
                    // them against the next client.
                    return Err(LspClientError::RequestTimeout {
                        method: "test".to_string(),
                        ms: 0,
                    });
                }
                let lock = client.lock().await;
                resync.store(lock.documents_snapshot().await.len(), Ordering::SeqCst);
                Ok(serde_json::Value::Null)
            })
        })
        .await
        .expect("retry succeeded");

    // Two attempts: first failed transiently, second succeeded.
    assert_eq!(attempt.load(Ordering::SeqCst), 2);
    // After the reconnect, the new client has had the open doc
    // replayed via did_open; documents_snapshot reflects it.
    assert_eq!(resync_count.load(Ordering::SeqCst), 1);
}

/// BL-076 — mock server that issues a `workspace/configuration`
/// request to the host right after `initialized` lands, captures
/// the response, and surfaces it back via a
/// `publishDiagnostics` notification so the test can assert the
/// host actually replied (rather than dropping the request as the
/// pre-fix code did).
fn write_config_probe_mock_server() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mock_lsp_probe.py");
    let body = r#"#!/usr/bin/env python3
"""BL-076 probe mock — fires workspace/configuration after init.

The host's reader task is supposed to acknowledge server-initiated
requests with a spec-compliant result. Pre-BL-076 the canned response
was discarded and we'd hang waiting on the server's stdin. This mock
threads the captured response out via publishDiagnostics so the test
can confirm both that:
  - we got a response (the read returned)
  - the response was the expected `[null, null]` shape
"""
import json
import sys
import threading


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


# Request-id 100 is reserved for the probe so it doesn't collide with
# host-initiated request ids (which start at 1 and grow monotonically).
PROBE_ID = 100
captured = {"value": None, "received": threading.Event()}


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
                        "capabilities": { "textDocumentSync": 1 },
                        "serverInfo": { "name": "mock-probe", "version": "0.0.1" },
                    },
                }
            )
        elif method == "initialized":
            # Fire the server-initiated request right after the
            # client confirms initialization. Two items so the host
            # has to compute the array length, not just emit a
            # constant.
            write_message(
                {
                    "jsonrpc": "2.0",
                    "id": PROBE_ID,
                    "method": "workspace/configuration",
                    "params": {
                        "items": [
                            { "section": "rust-analyzer.cargo" },
                            { "section": "rust-analyzer.checkOnSave" },
                        ]
                    },
                }
            )
        elif msg.get("id") == PROBE_ID:
            # The host's reply lands here as a regular Response.
            captured["value"] = msg.get("result")
            captured["received"].set()
            # Surface the captured response via publishDiagnostics so
            # the test can read it off the bus. The diagnostic message
            # is the JSON-encoded result.
            write_message(
                {
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": {
                        "uri": "file:///probe.mock",
                        "diagnostics": [
                            {
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": 0, "character": 1},
                                },
                                "severity": 3,
                                "message": json.dumps(captured["value"]),
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&path, perm).unwrap();
    }
    (dir, path)
}

#[tokio::test]
async fn server_initiated_workspace_configuration_request_is_answered() {
    // BL-076 — the host now answers `workspace/configuration` with
    // an array of nulls (one per requested item). Pre-fix this test
    // would hang because the canned response was discarded and the
    // server's stdin waited forever.
    if !python_available() {
        eprintln!("python3 not available — skipping");
        return;
    }
    let (_dir, script) = write_config_probe_mock_server();
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

    // Wait for the publishDiagnostics that surfaces the captured
    // response. A 5 s ceiling is generous — the probe fires
    // immediately after `initialized` and the round-trip is local
    // pipe IO.
    let pushed = timeout(Duration::from_secs(5), client.next_notification())
        .await
        .expect("notification arrived within 5s")
        .expect("channel still open");
    assert_eq!(pushed.method, "textDocument/publishDiagnostics");
    let msg = pushed.params["diagnostics"][0]["message"]
        .as_str()
        .expect("message is a string")
        .to_string();
    // The captured response is the JSON-encoded array. Two items
    // requested → two nulls.
    assert_eq!(msg, "[null, null]");

    drop(client);
}
