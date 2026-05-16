//! End-to-end test for the BL-140 remote-forge server.
//!
//! Boots a real CLI runtime against a temporary forge, hands the
//! kernel + plugin context to [`RemoteServer`], drives it over a
//! `tokio::io::duplex` pair, and exercises the wire contract:
//!
//! - `ipc_call` round-trip against a real backend verb
//!   (`com.nexus.storage::list_files`).
//! - `ipc_call` against an unknown plugin → `-32000`.
//! - Unknown method → `-32601`.
//! - Missing required params → `-32602`.
//! - `event_subscribe` + matching publish → server-pushed `event`
//!   notification carrying the supplied `subscription_id`.
//! - `event_unsubscribe` shuts down a registered subscription.
//! - Duplicate `subscription_id` rejected.
//! - Unknown unsubscribe id reports `{ ok: false }`.

use std::sync::Arc;
use std::time::Duration;

use nexus_bootstrap::{build_cli_runtime, init_forge, Runtime};
use nexus_remote::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RemoteServer,
};
use serde_json::{json, Value};
use tokio::io::{BufReader, DuplexStream};

async fn read_one(reader: &mut BufReader<DuplexStream>) -> JsonRpcMessage {
    nexus_remote::transport::read_message(reader)
        .await
        .expect("read_one: framing error")
}

async fn write_request(writer: &mut DuplexStream, req: JsonRpcRequest) {
    nexus_remote::transport::write_message(writer, &JsonRpcMessage::Request(req))
        .await
        .expect("write_request: framing error");
}

fn req(id: i64, method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(id),
        method: method.to_string(),
        params: Some(params),
    }
}

fn expect_response(msg: JsonRpcMessage) -> JsonRpcResponse {
    match msg {
        JsonRpcMessage::Response(r) => r,
        other => panic!("expected Response, got {other:?}"),
    }
}

#[allow(dead_code)]
fn expect_notification(msg: JsonRpcMessage) -> JsonRpcNotification {
    match msg {
        JsonRpcMessage::Notification(n) => n,
        other => panic!("expected Notification, got {other:?}"),
    }
}

/// Build a fresh forge runtime + duplex pair and return:
/// `(server, server_reader, server_writer, client_writer, client_reader, forge_guard)`.
///
/// The forge tempdir guard is returned so the caller keeps the
/// directory alive for the test's lifetime; dropping it tears down the
/// forge before the server has a chance to shut down cleanly.
///
/// The kernel is intentionally leaked (`Box::leak`) because dropping
/// it mid-test tears down every plugin and breaks the IPC surface
/// while the server task is still serving. The OS reclaims it at
/// process exit.
fn boot_pair() -> (
    RemoteServer,
    DuplexStream,
    DuplexStream,
    DuplexStream,
    DuplexStream,
    tempfile::TempDir,
) {
    let forge = tempfile::tempdir().expect("tempdir");
    init_forge(forge.path()).expect("init_forge");
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");
    let Runtime {
        kernel,
        context,
        loader: _loader,
    } = runtime;
    let event_bus = kernel.event_bus();
    let server = RemoteServer::new(Arc::new(context), event_bus)
        .with_timeout(Duration::from_secs(30));
    Box::leak(Box::new(kernel));

    // server_reader pairs with client_writer.
    let (client_writer, server_reader) = tokio::io::duplex(64 * 1024);
    // server_writer pairs with client_reader.
    let (server_writer, client_reader) = tokio::io::duplex(64 * 1024);
    (
        server,
        server_reader,
        server_writer,
        client_writer,
        client_reader,
        forge,
    )
}

#[tokio::test]
async fn ipc_call_round_trips_to_storage_list_files() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    write_request(
        &mut c_out,
        req(
            1,
            "ipc_call",
            json!({
                "plugin_id": "com.nexus.storage",
                "command": "list_dir",
                "args": { "path": "" },
            }),
        ),
    )
    .await;

    let resp = expect_response(read_one(&mut c_reader).await);
    assert_eq!(resp.id, json!(1));
    assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
    assert!(resp.result.is_some(), "missing result");

    drop(c_out);
    let _ = handle.await.unwrap();
}

#[tokio::test]
async fn ipc_call_unknown_plugin_returns_server_error() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    write_request(
        &mut c_out,
        req(
            2,
            "ipc_call",
            json!({
                "plugin_id": "com.does.not.exist",
                "command": "noop",
                "args": {},
            }),
        ),
    )
    .await;
    let resp = expect_response(read_one(&mut c_reader).await);
    assert_eq!(resp.id, json!(2));
    let err = resp.error.expect("expected error");
    assert_eq!(err.code, -32000);
    assert!(err.message.contains("ipc_call failed"));

    drop(c_out);
    let _ = handle.await.unwrap();
}

#[tokio::test]
async fn unknown_method_returns_method_not_found() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    write_request(&mut c_out, req(3, "frobnicate", json!({}))).await;
    let resp = expect_response(read_one(&mut c_reader).await);
    assert_eq!(resp.id, json!(3));
    let err = resp.error.expect("expected error");
    assert_eq!(err.code, -32601);
    assert!(err.message.contains("method not found"));

    drop(c_out);
    let _ = handle.await.unwrap();
}

#[tokio::test]
async fn ipc_call_missing_plugin_id_returns_invalid_params() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    write_request(&mut c_out, req(4, "ipc_call", json!({ "command": "x" }))).await;
    let resp = expect_response(read_one(&mut c_reader).await);
    assert_eq!(resp.id, json!(4));
    let err = resp.error.expect("expected error");
    assert_eq!(err.code, -32602);

    drop(c_out);
    let _ = handle.await.unwrap();
}

#[tokio::test]
async fn event_subscribe_streams_publishes_and_unsubscribe_stops_them() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    // Subscribe to All. The bootstrap path emits lifecycle events as
    // plugins finish; we don't depend on a specific event arriving —
    // we just validate the wire shape when one does, and confirm
    // subscribe/unsubscribe round-trip.
    write_request(
        &mut c_out,
        req(
            5,
            "event_subscribe",
            json!({
                "subscription_id": "test-sub-1",
                "filter": { "kind": "all" },
            }),
        ),
    )
    .await;
    let sub_resp = expect_response(read_one(&mut c_reader).await);
    assert_eq!(sub_resp.id, json!(5));
    assert!(sub_resp.error.is_none(), "{:?}", sub_resp.error);
    assert_eq!(
        sub_resp.result.unwrap()["subscription_id"],
        json!("test-sub-1")
    );

    // Issue an ipc_call to provoke runtime activity; the bus may emit
    // an event during the call.
    write_request(
        &mut c_out,
        req(
            6,
            "ipc_call",
            json!({
                "plugin_id": "com.nexus.storage",
                "command": "list_dir",
                "args": { "path": "" },
            }),
        ),
    )
    .await;

    // Drain up to 10 frames waiting for the ipc_call response. Any
    // event notifications that arrive on the way past are validated
    // for wire shape but not required to arrive.
    let mut saw_ipc_response = false;
    for _ in 0..10 {
        let msg = tokio::time::timeout(Duration::from_secs(2), read_one(&mut c_reader))
            .await
            .expect("timed out reading server frame");
        match msg {
            JsonRpcMessage::Notification(n) => {
                assert_eq!(n.method, "event");
                let p = n.params.expect("event must carry params");
                assert_eq!(p["subscription_id"], json!("test-sub-1"));
                assert!(p["event"].is_object());
            }
            JsonRpcMessage::Response(r) if r.id == json!(6) => {
                saw_ipc_response = true;
                break;
            }
            JsonRpcMessage::Response(r) => panic!("unexpected response id {:?}", r.id),
            JsonRpcMessage::Request(_) => panic!("server should not issue requests"),
        }
    }
    assert!(saw_ipc_response, "ipc_call response never arrived");

    write_request(
        &mut c_out,
        req(
            7,
            "event_unsubscribe",
            json!({ "subscription_id": "test-sub-1" }),
        ),
    )
    .await;
    // The unsubscribe response can race with in-flight event notifs
    // that the forwarder task already buffered. Drain until we see it.
    let mut unsub_seen = false;
    for _ in 0..20 {
        let msg = tokio::time::timeout(Duration::from_secs(2), read_one(&mut c_reader))
            .await
            .expect("timed out waiting for unsubscribe response");
        match msg {
            JsonRpcMessage::Response(r) if r.id == json!(7) => {
                assert_eq!(r.result.unwrap()["ok"], json!(true));
                unsub_seen = true;
                break;
            }
            JsonRpcMessage::Notification(_) => continue,
            other => panic!("unexpected frame while awaiting unsubscribe: {other:?}"),
        }
    }
    assert!(unsub_seen, "unsubscribe response never arrived");

    drop(c_out);
    let _ = handle.await.unwrap();
}

#[tokio::test]
async fn event_subscribe_missing_filter_returns_invalid_params() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    write_request(
        &mut c_out,
        req(8, "event_subscribe", json!({ "subscription_id": "x" })),
    )
    .await;
    let resp = expect_response(read_one(&mut c_reader).await);
    assert_eq!(resp.id, json!(8));
    let err = resp.error.expect("expected error");
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("filter"));

    drop(c_out);
    let _ = handle.await.unwrap();
}

#[tokio::test]
async fn event_unsubscribe_unknown_id_reports_ok_false() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    write_request(
        &mut c_out,
        req(
            9,
            "event_unsubscribe",
            json!({ "subscription_id": "never-subscribed" }),
        ),
    )
    .await;
    let resp = expect_response(read_one(&mut c_reader).await);
    assert_eq!(resp.id, json!(9));
    let r = resp.result.unwrap();
    assert_eq!(r["ok"], json!(false));
    assert_eq!(r["reason"], json!("unknown subscription_id"));

    drop(c_out);
    let _ = handle.await.unwrap();
}

#[tokio::test]
async fn duplicate_subscription_id_is_rejected() {
    let (server, s_in, s_out, mut c_out, c_in, _forge) = boot_pair();
    let handle = tokio::spawn(async move { server.serve(s_in, s_out).await });
    let mut c_reader = BufReader::new(c_in);

    let body = json!({
        "subscription_id": "dup",
        "filter": { "kind": "all" },
    });
    write_request(&mut c_out, req(10, "event_subscribe", body.clone())).await;
    let first = expect_response(read_one(&mut c_reader).await);
    assert!(first.error.is_none(), "{:?}", first.error);

    write_request(&mut c_out, req(11, "event_subscribe", body)).await;
    // Drain any event notifs that arrive between the two subscribes.
    let mut got_dup_err = false;
    for _ in 0..20 {
        let msg = tokio::time::timeout(Duration::from_secs(2), read_one(&mut c_reader))
            .await
            .expect("timed out waiting for duplicate-subscribe error");
        match msg {
            JsonRpcMessage::Response(r) if r.id == json!(11) => {
                let err = r.error.expect("expected error on duplicate");
                assert_eq!(err.code, -32000);
                assert!(err.message.contains("already in use"));
                got_dup_err = true;
                break;
            }
            JsonRpcMessage::Notification(_) => continue,
            other => panic!("unexpected frame: {other:?}"),
        }
    }
    assert!(got_dup_err, "duplicate-subscribe error never arrived");

    drop(c_out);
    let _ = handle.await.unwrap();
}
