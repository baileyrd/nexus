//! Phase 2a integration test — wires a [`RemoteClient`] to an
//! in-process [`RemoteServer`] over `tokio::io::duplex` and exercises
//! every public client method against a real CLI runtime.
//!
//! This test deliberately does NOT exercise SSH child-process
//! spawning; that's gated on Phase 2b (the `build_remote_runtime`
//! factory) and adds an external dependency on the `ssh` binary that
//! makes the test hostile to CI. The duplex shim proves the wire
//! contract works; SSH is "just" the transport.

use std::sync::Arc;
use std::time::Duration;

use nexus_bootstrap::{build_cli_runtime, init_forge, Runtime};
use nexus_remote::{EventDelivery, RemoteClient, RemoteClientError, RemoteServer};
use serde_json::{json, Value};
use tokio::io::AsyncWrite;
use tokio::sync::mpsc;

/// Boots a real CLI runtime + spawns a server task wired through
/// duplex, and returns a `RemoteClient` bound to the client-facing
/// halves. The kernel is leaked for the test's lifetime — dropping it
/// tears down every plugin and breaks the IPC surface mid-test.
async fn boot_pair() -> (RemoteClient, tempfile::TempDir, tokio::task::JoinHandle<()>) {
    let forge = tempfile::tempdir().expect("tempdir");
    init_forge(forge.path()).expect("init_forge");
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");
    let Runtime {
        kernel,
        context,
        loader: _loader,
    } = runtime;
    let event_bus = kernel.event_bus();
    let server =
        RemoteServer::new(Arc::new(context), event_bus).with_timeout(Duration::from_secs(30));
    Box::leak(Box::new(kernel));

    // client_writer → server_reader (client outbound).
    let (client_writer, server_reader) = tokio::io::duplex(64 * 1024);
    // server_writer → client_reader (client inbound).
    let (server_writer, client_reader) = tokio::io::duplex(64 * 1024);

    let server_handle = tokio::spawn(async move {
        server
            .serve(server_reader, server_writer)
            .await
            .expect("server serve");
    });

    let writer_boxed: Box<dyn AsyncWrite + Unpin + Send> = Box::new(client_writer);
    let client = RemoteClient::new(client_reader, writer_boxed)
        .with_default_timeout(Duration::from_secs(30));
    (client, forge, server_handle)
}

#[tokio::test]
async fn ipc_call_round_trips_through_the_client() {
    let (client, _forge, _server) = boot_pair().await;

    let v: Value = client
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "relpath": "" }),
            None,
        )
        .await
        .expect("ipc_call");

    // The exact shape is plugin-defined; we just need a Value back.
    assert!(!v.is_null(), "list_dir should return some payload");
    client.shutdown().await;
}

#[tokio::test]
async fn ipc_call_unknown_command_surfaces_server_error() {
    let (client, _forge, _server) = boot_pair().await;

    let err = client
        .ipc_call("com.nexus.storage", "no_such_command", json!({}), None)
        .await
        .unwrap_err();
    match err {
        RemoteClientError::Server { code, message } => {
            assert_eq!(code, -32000);
            assert!(
                message.contains("ipc_call failed"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected Server, got {other:?}"),
    }
    client.shutdown().await;
}

#[tokio::test]
async fn ipc_call_with_per_call_timeout_overrides_default() {
    let (client, _forge, _server) = boot_pair().await;

    // A 1ms timeout against any real verb is racy by design — we
    // simply confirm the override path runs without a panic and
    // either succeeds (fast verb wins the race) or times out cleanly.
    let outcome = client
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "relpath": "" }),
            Some(Duration::from_millis(1)),
        )
        .await;
    match outcome {
        Ok(_) => {}
        Err(RemoteClientError::Timeout(_)) => {}
        Err(other) => panic!("unexpected outcome: {other:?}"),
    }
    client.shutdown().await;
}

#[tokio::test]
async fn subscribe_unsubscribe_round_trips() {
    let (client, _forge, _server) = boot_pair().await;

    let (tx, mut rx) = mpsc::unbounded_channel::<EventDelivery>();
    let echoed = client
        .subscribe("test-sub", json!({"kind": "all"}), tx)
        .await
        .expect("subscribe");
    assert_eq!(echoed, "test-sub");

    // Provoke runtime activity so the bus has something to emit. We
    // don't depend on a specific event — only that we either see one
    // (validating delivery wire shape) or don't (still OK).
    let _ = client
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "relpath": "" }),
            None,
        )
        .await
        .expect("ipc_call");

    // Drain any deliveries that arrived during the call.
    if let Ok(delivery) = tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
        let d = delivery.expect("subscription channel still open");
        assert_eq!(d.subscription_id, "test-sub");
        assert!(d.event.is_object());
    }

    let ok = client.unsubscribe("test-sub").await.expect("unsubscribe");
    assert!(ok, "server should ack a real subscription");
    client.shutdown().await;
}

#[tokio::test]
async fn subscribe_with_invalid_filter_surfaces_server_error() {
    let (client, _forge, _server) = boot_pair().await;

    let (tx, _rx) = mpsc::unbounded_channel::<EventDelivery>();
    let err = client
        .subscribe("bad-filter-sub", json!({"kind": "frobnicate"}), tx)
        .await
        .unwrap_err();
    match err {
        RemoteClientError::Server { code, message } => {
            assert_eq!(code, -32602);
            assert!(message.contains("frobnicate"), "got: {message}");
        }
        other => panic!("expected Server, got {other:?}"),
    }
    client.shutdown().await;
}

#[tokio::test]
async fn unsubscribe_unknown_id_returns_ok_false() {
    let (client, _forge, _server) = boot_pair().await;

    let ok = client
        .unsubscribe("never-subscribed")
        .await
        .expect("unsubscribe call");
    assert!(!ok);
    client.shutdown().await;
}

#[tokio::test]
async fn duplicate_subscription_id_is_rejected_and_first_stays_active() {
    let (client, _forge, _server) = boot_pair().await;

    let (tx1, _rx1) = mpsc::unbounded_channel::<EventDelivery>();
    let echoed = client
        .subscribe("dup", json!({"kind": "all"}), tx1)
        .await
        .expect("first subscribe");
    assert_eq!(echoed, "dup");

    let (tx2, _rx2) = mpsc::unbounded_channel::<EventDelivery>();
    let err = client
        .subscribe("dup", json!({"kind": "all"}), tx2)
        .await
        .unwrap_err();
    match err {
        RemoteClientError::Server { code, message } => {
            assert_eq!(code, -32000);
            assert!(message.contains("already in use"), "got: {message}");
        }
        other => panic!("expected Server, got {other:?}"),
    }

    // First subscription must still work — unsubscribe should still
    // return ok=true.
    let ok = client.unsubscribe("dup").await.expect("unsubscribe");
    assert!(ok);
    client.shutdown().await;
}

#[tokio::test]
async fn shutdown_wakes_pending_calls_via_router_drop() {
    let (client, _forge, _server) = boot_pair().await;
    // Make one normal call to confirm the channel works.
    let _ = client
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "relpath": "" }),
            None,
        )
        .await
        .expect("first call");

    client.shutdown().await;

    // After shutdown, subsequent calls should fail fast — the router
    // is gone so the response oneshot can't be answered.
    let err = client
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "relpath": "" }),
            Some(Duration::from_secs(1)),
        )
        .await
        .unwrap_err();
    // Either Timeout (because nothing answers) or RouterStopped is
    // acceptable — both indicate the connection is unusable.
    match err {
        RemoteClientError::Timeout(_) | RemoteClientError::RouterStopped => {}
        other => panic!("unexpected outcome after shutdown: {other:?}"),
    }
}
