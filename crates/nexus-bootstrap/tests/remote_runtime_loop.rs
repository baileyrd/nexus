//! BL-140 Phase 2b integration test — drives a `RemoteRuntime` over a
//! `tokio::io::duplex` pair against an in-process `RemoteServer`
//! bound to a real CLI runtime, and exercises the `IpcInvoker` trait
//! end-to-end.
//!
//! This is the local↔remote equivalent of the Phase 1 e2e test +
//! Phase 2a client/server loop, but wired through the
//! `build_remote_runtime_over_pipes` factory so it covers the same
//! call path the CLI's `--forge-path ssh://` will follow once Phase
//! 2b lifts everything into the App.

use std::sync::Arc;
use std::time::Duration;

use nexus_bootstrap::{
    build_cli_runtime, init_forge,
    invoker::IpcInvokerError,
    remote::{build_remote_runtime_over_pipes, NoopTransportGuard},
    Runtime,
};
use nexus_remote::RemoteServer;
use serde_json::json;
use tokio::io::AsyncWrite;

/// Boot a server task against a real CLI runtime + paired duplexes,
/// and build a `RemoteRuntime` against the matching client halves.
///
/// Returns the runtime + the forge tempdir guard + the server's
/// JoinHandle so the test can keep the server alive for its scope.
fn boot(
) -> (
    nexus_bootstrap::remote::RemoteRuntime,
    tempfile::TempDir,
    tokio::task::JoinHandle<()>,
) {
    let forge = tempfile::tempdir().expect("tempdir");
    init_forge(forge.path()).expect("init_forge");
    let local = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");
    let Runtime {
        kernel,
        context,
        loader: _loader,
    } = local;
    let event_bus = kernel.event_bus();
    // Leak so plugins stay live for the test.
    Box::leak(Box::new(kernel));

    let server = RemoteServer::new(Arc::new(context), event_bus)
        .with_timeout(Duration::from_secs(30));

    let (client_writer, server_reader) = tokio::io::duplex(64 * 1024);
    let (server_writer, client_reader) = tokio::io::duplex(64 * 1024);

    let server_handle = tokio::spawn(async move {
        server
            .serve(server_reader, server_writer)
            .await
            .expect("server serve");
    });

    let writer_boxed: Box<dyn AsyncWrite + Unpin + Send> = Box::new(client_writer);
    let runtime = build_remote_runtime_over_pipes(
        client_reader,
        writer_boxed,
        Box::new(NoopTransportGuard),
    );
    (runtime, forge, server_handle)
}

#[tokio::test]
async fn invoker_routes_ipc_call_over_the_remote_loop() {
    let (rt, _forge, _server) = boot();
    let invoker = rt.invoker();

    let v = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("ipc_call");
    assert!(!v.is_null(), "list_dir should return some payload");
    rt.shutdown().await;
}

#[tokio::test]
async fn invoker_surfaces_remote_server_errors_with_jsonrpc_code() {
    let (rt, _forge, _server) = boot();
    let invoker = rt.invoker();

    let err = invoker
        .ipc_call(
            "com.nexus.storage",
            "no_such_command",
            json!({}),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    match err {
        IpcInvokerError::Remote { code, message } => {
            assert_eq!(code, -32000);
            assert!(
                message.contains("ipc_call failed"),
                "unexpected: {message}"
            );
        }
        other => panic!("expected Remote, got: {other}"),
    }
    rt.shutdown().await;
}

#[tokio::test]
async fn invoker_timeout_returns_typed_timeout_variant() {
    let (rt, _forge, _server) = boot();
    let invoker = rt.invoker();

    // Pick a verb that exists but might race a 1ms deadline. We
    // accept Ok (the call wins) or Timeout (the deadline wins). Other
    // outcomes are bugs.
    let outcome = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_millis(1),
        )
        .await;
    match outcome {
        Ok(_) => {}
        Err(IpcInvokerError::Timeout {
            plugin_id,
            command,
            timeout_ms,
        }) => {
            assert_eq!(plugin_id, "com.nexus.storage");
            assert_eq!(command, "list_dir");
            assert_eq!(timeout_ms, 1);
        }
        Err(other) => panic!("unexpected: {other}"),
    }
    rt.shutdown().await;
}

#[tokio::test]
async fn shutdown_makes_subsequent_calls_fail_fast() {
    let (rt, _forge, _server) = boot();
    let invoker = rt.invoker();

    // Confirm the runtime works at least once.
    let _ = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("first call");

    rt.shutdown().await;

    let outcome = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_millis(500),
        )
        .await;
    match outcome {
        Err(IpcInvokerError::Timeout { .. }) | Err(IpcInvokerError::Transport(_)) => {}
        other => panic!("expected post-shutdown failure, got: {other:?}"),
    }
}
