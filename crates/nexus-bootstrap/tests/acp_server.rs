//! BL-145 / Hermes Feature 7 — integration tests for the inbound
//! `AcpServer` against a booted runtime over an in-process tokio
//! duplex pipe.
//!
//! Covers the five DoD scenarios from the BL entry: happy-path
//! request round-trip, unknown-method `-32601`, invalid-params kernel
//! error surface (`-32000`), pipelined requests preserve response
//! order, and graceful disconnect on reader EOF.

#![cfg(not(target_arch = "wasm32"))]

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use common::MinimalForge;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Run a closure that drives the inbound side of an `AcpServer`
/// running concurrently on the same task. The server future and the
/// closure are joined with `tokio::join!`; the closure exits when
/// it's done writing requests + reading responses, and dropping its
/// writer triggers the server to see EOF and return `Ok(())`.
///
/// Takes ownership of the `MinimalForge` so we can destructure its
/// `Runtime` and move the `KernelPluginContext` into an `Arc`. The
/// tempdir lives inside the forge for the duration of the join, so
/// no file-locking races with a second runtime build.
async fn drive_server<F, Fut>(forge: MinimalForge, client: F)
where
    F: FnOnce(
            tokio::io::DuplexStream,
            BufReader<tokio::io::DuplexStream>,
        ) -> Fut
        + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let MinimalForge {
        runtime,
        tempdir: _tempdir,
    } = forge;
    let nexus_bootstrap::Runtime {
        kernel: _kernel,
        context,
        loader: _loader,
    } = runtime;
    let context = Arc::new(context);
    let server = nexus_acp::AcpServer::new(Arc::clone(&context))
        .with_timeout(Duration::from_secs(10));

    let (client_writer, server_reader) = tokio::io::duplex(64 * 1024);
    let (server_writer, client_reader_inner) = tokio::io::duplex(64 * 1024);
    let client_reader = BufReader::new(client_reader_inner);

    let server_fut = async move {
        server
            .serve(server_reader, server_writer)
            .await
            .expect("server should exit cleanly");
    };
    let client_fut = client(client_writer, client_reader);
    tokio::join!(server_fut, client_fut);
}

/// Write one JSON-RPC request frame, then read one line back.
async fn round_trip(
    writer: &mut tokio::io::DuplexStream,
    reader: &mut BufReader<tokio::io::DuplexStream>,
    request: &Value,
) -> Value {
    let mut body = serde_json::to_vec(request).unwrap();
    body.push(b'\n');
    writer.write_all(&body).await.unwrap();
    writer.flush().await.unwrap();
    let mut line = String::new();
    let n = reader.read_line(&mut line).await.unwrap();
    assert!(n > 0, "server should respond to {request}");
    serde_json::from_str(line.trim()).expect("response is valid JSON")
}

#[tokio::test(flavor = "current_thread")]
async fn happy_path_agent_list_routes_through_ipc() {
    let forge = MinimalForge::new();
    drive_server(forge, |mut writer, mut reader| async move {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "agent/list",
            "params": {}
        });
        let resp = round_trip(&mut writer, &mut reader, &req).await;
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        // `agent/list` routes to `com.nexus.agent::session_list` which
        // returns an array (empty in a fresh forge).
        let result = resp.get("result").cloned().unwrap_or(Value::Null);
        assert!(
            result.is_array() || result.is_object(),
            "session_list should return JSON, got {result}",
        );
        drop(writer);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_method_returns_minus_32601() {
    let forge = MinimalForge::new();
    drive_server(forge, |mut writer, mut reader| async move {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "agent/nonexistent",
            "params": {}
        });
        let resp = round_trip(&mut writer, &mut reader, &req).await;
        assert_eq!(resp["id"], 99);
        let err = resp.get("error").expect("unknown method must produce an error");
        assert_eq!(err["code"], -32601);
        assert!(err["message"].as_str().unwrap().contains("agent/nonexistent"));
        assert!(resp.get("result").is_none(), "error responses omit result");
        drop(writer);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_params_for_known_method_surface_as_server_error() {
    let forge = MinimalForge::new();
    drive_server(forge, |mut writer, mut reader| async move {
        // `agent/get` routes to `com.nexus.agent::session_get` which
        // requires a `session_id` field. Sending an empty params object
        // tunnels the kernel error back as `-32000 server error`.
        let req = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "agent/get",
            "params": {}
        });
        let resp = round_trip(&mut writer, &mut reader, &req).await;
        assert_eq!(resp["id"], 5);
        let err = resp.get("error").expect("missing field should produce an error");
        assert_eq!(err["code"], -32000, "ipc failures surface as -32000");
        assert!(
            err["message"].as_str().unwrap().contains("server error"),
            "got: {err}",
        );
        drop(writer);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pipelined_requests_preserve_response_order() {
    let forge = MinimalForge::new();
    drive_server(forge, |mut writer, mut reader| async move {
        for i in 0..3 {
            let req = json!({
                "jsonrpc": "2.0",
                "id": i,
                "method": "agent/nonexistent",
                "params": {}
            });
            let mut body = serde_json::to_vec(&req).unwrap();
            body.push(b'\n');
            writer.write_all(&body).await.unwrap();
        }
        writer.flush().await.unwrap();
        for expected in 0..3 {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let resp: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(resp["id"], expected, "response order preserved");
            assert!(resp.get("error").is_some());
        }
        drop(writer);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn graceful_disconnect_returns_ok() {
    let forge = MinimalForge::new();
    drive_server(forge, |writer, _reader| async move {
        // Drop the writer immediately — server sees EOF and exits Ok.
        drop(writer);
    })
    .await;
}

#[test]
fn route_table_uses_kernel_agent_plugin_id() {
    // Pure unit reach-through — proves the routing table this BL
    // ships against is the same `com.nexus.agent` id the agent core
    // plugin registers under (via `nexus_bootstrap::register_core_plugins`).
    let agent_id = match nexus_acp::server::route_method("agent/run") {
        nexus_acp::server::RoutedMethod::Known { plugin_id, .. } => plugin_id,
        nexus_acp::server::RoutedMethod::Unknown => panic!("agent/run must be on the allow-list"),
    };
    assert_eq!(agent_id, "com.nexus.agent");
}
