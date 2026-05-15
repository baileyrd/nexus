//! End-to-end test against a tiny mock DAP adapter (Python).
//!
//! The mock script speaks the DAP framing well enough to satisfy:
//!
//! - Process spawn / handshake (`initialize` → adapter capabilities)
//! - `launch` request → success response
//! - `setBreakpoints` → verified breakpoint reply
//! - `configurationDone` → ack + adapter emits a `stopped` event
//! - `stackTrace` → one synthetic frame
//! - `continue` → adapter emits `terminated`
//! - `disconnect` → graceful exit
//!
//! That's enough surface to exercise:
//!
//! - Spawn / framing / handshake
//! - Request → response correlation by `request_seq`
//! - Adapter-pushed event fan-out
//! - Graceful shutdown
//! - Breakpoint cache survives a round-trip
//!
//! If `python3` isn't on `$PATH` the test is silently skipped so CI
//! runners without Python stay green.

use std::path::PathBuf;
use std::time::Duration;

use nexus_dap::{DapClient, DapAdapterSpec};
use serde_json::json;
use tokio::time::timeout;

fn write_mock_adapter() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mock_dap.py");
    let body = r#"#!/usr/bin/env python3
"""Tiny stdio DAP adapter used by nexus-dap's end_to_end test."""
import json
import sys
import threading

_lock = threading.Lock()
_seq = 0


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
    with _lock:
        sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
        sys.stdout.buffer.write(body)
        sys.stdout.buffer.flush()


def next_seq():
    global _seq
    _seq += 1
    return _seq


def respond(req, body=None, success=True, message=None):
    payload = {
        "seq": next_seq(),
        "type": "response",
        "request_seq": req["seq"],
        "success": success,
        "command": req["command"],
    }
    if body is not None:
        payload["body"] = body
    if message is not None:
        payload["message"] = message
    write_message(payload)


def event(name, body=None):
    payload = {"seq": next_seq(), "type": "event", "event": name}
    if body is not None:
        payload["body"] = body
    write_message(payload)


def main():
    while True:
        msg = read_message()
        if msg is None:
            return
        cmd = msg.get("command")
        if cmd == "initialize":
            respond(
                msg,
                body={
                    "supportsConfigurationDoneRequest": True,
                    "supportsFunctionBreakpoints": True,
                    "supportsConditionalBreakpoints": True,
                    "supportsTerminateRequest": True,
                },
            )
        elif cmd == "launch":
            respond(msg, body={})
            # Spec: emit `initialized` after `launch`.
            event("initialized")
        elif cmd == "setBreakpoints":
            bps = msg.get("arguments", {}).get("breakpoints", [])
            respond(
                msg,
                body={
                    "breakpoints": [
                        {"verified": True, "line": b.get("line")} for b in bps
                    ]
                },
            )
        elif cmd == "configurationDone":
            respond(msg, body={})
            event("stopped", body={"reason": "breakpoint", "threadId": 1})
        elif cmd == "threads":
            respond(msg, body={"threads": [{"id": 1, "name": "main"}]})
        elif cmd == "stackTrace":
            respond(
                msg,
                body={
                    "stackFrames": [
                        {
                            "id": 1,
                            "name": "main",
                            "line": 7,
                            "column": 1,
                            "source": {"path": "/tmp/x.rs"},
                        }
                    ],
                    "totalFrames": 1,
                },
            )
        elif cmd == "continue":
            respond(msg, body={"allThreadsContinued": True})
            event("terminated")
        elif cmd == "disconnect":
            respond(msg, body={})
            return
        elif cmd is not None:
            respond(msg, success=False, message=f"unsupported {cmd}")


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

fn python_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// BL-081 live smoke — locate a Python interpreter with `debugpy`
/// installed. Honours `$NEXUS_DAP_LIVE_DEBUGPY_PYTHON` (full path to
/// a python binary in a venv where `pip install debugpy` has been
/// run); falls back to probing `python3` on `$PATH`. Returns `None`
/// when no working interpreter is found so the live-smoke test
/// skips silently on machines without the adapter installed.
fn find_debugpy_python() -> Option<String> {
    let candidates: Vec<String> = std::env::var("NEXUS_DAP_LIVE_DEBUGPY_PYTHON")
        .ok()
        .into_iter()
        .chain(std::iter::once("python3".to_string()))
        .collect();
    for c in candidates {
        let ok = std::process::Command::new(&c)
            .args(["-c", "import debugpy; raise SystemExit(0)"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(c);
        }
    }
    None
}

#[tokio::test]
async fn full_debug_session_lifecycle() {
    if !python_available() {
        eprintln!("python3 not available — skipping");
        return;
    }
    let (_dir, script) = write_mock_adapter();
    let spec = DapAdapterSpec {
        name: "mock".to_string(),
        command: "python3".to_string(),
        args: vec![script.to_string_lossy().into_owned()],
        adapter_type: Some("mock".to_string()),
        file_types: vec!["mock".to_string()],
        disabled: false,
        env: Default::default(),
    };

    let client = timeout(
        Duration::from_secs(15),
        DapClient::connect("mock", &spec),
    )
    .await
    .expect("connect did not deadlock")
    .expect("handshake succeeded");

    // Capabilities captured from initialize.
    let caps = client.capabilities().await;
    assert!(caps.supports_configuration_done);
    assert!(caps.supports_function_breakpoints);
    assert!(caps.supports_conditional_breakpoints);
    assert!(caps.supports_terminate_request);

    // launch → success + `initialized` event.
    let _ = client
        .send_request("launch", Some(json!({"program": "/tmp/x"})))
        .await
        .expect("launch ok");
    let initialized = timeout(Duration::from_secs(2), client.next_event())
        .await
        .expect("event arrived")
        .expect("channel open");
    assert_eq!(initialized.event, "initialized");

    // setBreakpoints → verified replies. Cache locally too so we can
    // exercise the resync snapshot.
    let bps = json!({
        "source": { "path": "/tmp/x.rs" },
        "breakpoints": [{"line": 7}, {"line": 11}],
    });
    let resp = client
        .send_request("setBreakpoints", Some(bps))
        .await
        .expect("setBreakpoints ok")
        .expect("body present");
    let arr = resp["breakpoints"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["verified"], json!(true));
    assert_eq!(arr[0]["line"], json!(7));
    client
        .remember_breakpoints(
            "/tmp/x.rs",
            vec![
                nexus_dap::SourceBreakpointSpec {
                    line: 7,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                },
                nexus_dap::SourceBreakpointSpec {
                    line: 11,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                },
            ],
        )
        .await;
    let snap = client.breakpoints_snapshot().await;
    assert_eq!(snap.get("/tmp/x.rs").map(Vec::len), Some(2));

    // configurationDone → stopped event.
    let _ = client
        .send_request("configurationDone", None)
        .await
        .expect("configurationDone ok");
    let stopped = timeout(Duration::from_secs(2), client.next_event())
        .await
        .expect("event arrived")
        .expect("channel open");
    assert_eq!(stopped.event, "stopped");
    assert_eq!(stopped.body["reason"], json!("breakpoint"));
    assert_eq!(stopped.body["threadId"], json!(1));

    // threads → list.
    let threads = client
        .send_request("threads", None)
        .await
        .expect("threads ok")
        .expect("body");
    assert_eq!(threads["threads"].as_array().unwrap().len(), 1);
    assert_eq!(threads["threads"][0]["id"], json!(1));

    // stackTrace → one frame.
    let trace = client
        .send_request("stackTrace", Some(json!({"threadId": 1})))
        .await
        .expect("stackTrace ok")
        .expect("body");
    assert_eq!(trace["totalFrames"], json!(1));
    assert_eq!(trace["stackFrames"][0]["name"], json!("main"));

    // continue → terminated event.
    let _ = client
        .send_request("continue", Some(json!({"threadId": 1})))
        .await
        .expect("continue ok");
    let terminated = timeout(Duration::from_secs(2), client.next_event())
        .await
        .expect("event arrived")
        .expect("channel open");
    assert_eq!(terminated.event, "terminated");

    // disconnect — adapter exits cleanly.
    let _ = client
        .send_request("disconnect", Some(json!({"terminateDebuggee": true})))
        .await
        .expect("disconnect ok");
    drop(client);
}

#[tokio::test]
async fn drain_events_returns_queued_batch() {
    if !python_available() {
        eprintln!("python3 not available — skipping");
        return;
    }
    let (_dir, script) = write_mock_adapter();
    let spec = DapAdapterSpec {
        name: "mock".to_string(),
        command: "python3".to_string(),
        args: vec![script.to_string_lossy().into_owned()],
        adapter_type: Some("mock".to_string()),
        file_types: vec!["mock".to_string()],
        disabled: false,
        env: Default::default(),
    };
    let client = timeout(
        Duration::from_secs(15),
        DapClient::connect("mock", &spec),
    )
    .await
    .unwrap()
    .unwrap();

    // launch → emits `initialized`. configurationDone → emits `stopped`.
    let _ = client
        .send_request("launch", Some(json!({"program": "/tmp/x"})))
        .await
        .unwrap();
    let _ = client.send_request("configurationDone", None).await.unwrap();
    // Give the reader task time to put both events on the channel.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let drained = client.drain_events().await;
    assert!(!drained.is_empty(), "expected at least 1 event, got 0");
    let kinds: Vec<&str> = drained.iter().map(|e| e.event.as_str()).collect();
    // Order is preserved by the mpsc channel.
    assert!(kinds.contains(&"initialized") || kinds.contains(&"stopped"));
}

/// BL-081 live smoke — exercise the host against a real upstream
/// `debugpy.adapter` (Python's DAP server, the same one VS Code uses).
///
/// Scope is deliberately narrow: initialize handshake, capability
/// negotiation, and clean disconnect. Going further (launch /
/// setBreakpoints / continue / stack inspection) needs a target
/// Python program and deterministic stop semantics, which the mock
/// adapter already covers exhaustively. The value of this test is
/// confirming that an *unmodified* upstream adapter speaks the same
/// envelope our framing/parsing assumes — protecting against subtle
/// regressions in `transport.rs` / `protocol.rs` / `client.rs` that
/// our hand-rolled mock would mirror by construction.
///
/// Silently skipped on machines without `debugpy` installed.
#[tokio::test]
async fn live_smoke_debugpy_initialize_handshake() {
    let Some(py) = find_debugpy_python() else {
        eprintln!("debugpy not installed on any candidate Python — skipping live smoke");
        return;
    };
    let spec = DapAdapterSpec {
        name: "debugpy".to_string(),
        command: py,
        args: vec!["-m".to_string(), "debugpy.adapter".to_string()],
        adapter_type: Some("python".to_string()),
        file_types: vec!["py".to_string()],
        disabled: false,
        env: Default::default(),
    };

    let client = timeout(
        Duration::from_secs(15),
        DapClient::connect("debugpy", &spec),
    )
    .await
    .expect("debugpy connect did not deadlock")
    .expect("debugpy initialize succeeded");

    let caps = client.capabilities().await;
    // Upstream debugpy ships these in every release we'd realistically
    // exercise; if one drops, capability negotiation regressed and a
    // user-facing feature (e.g. function breakpoints UI in the panel)
    // would silently degrade.
    assert!(
        caps.supports_configuration_done,
        "debugpy must advertise configurationDone",
    );
    assert!(
        caps.supports_function_breakpoints,
        "debugpy must advertise function breakpoints",
    );

    // Don't `launch` — that needs a real target script and a stop
    // hook we'd have to author here. The handshake + caps assertion
    // is the load-bearing live-smoke signal.
    let _ = client
        .send_request("disconnect", Some(json!({"terminateDebuggee": false})))
        .await;
    drop(client);
}
