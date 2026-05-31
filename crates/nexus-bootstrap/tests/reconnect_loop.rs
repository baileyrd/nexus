//! BL-140 Phase 2c integration test — exercises the
//! [`ReconnectingRuntime`] against a `ConnectionFactory` that
//! pre-stages multiple test transports (paired duplexes wired to live
//! `RemoteServer` instances) and verifies the reconnect path recovers
//! after the first transport dies.
//!
//! Real SSH-child reconnect is impractical to test in CI (would
//! require sshd on the host); the contract this test pins is the
//! "Transport failure → tear down → rebuild via factory → retry"
//! flow that the SSH path inherits unchanged.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use nexus_bootstrap::invoker::IpcInvokerError;
use nexus_bootstrap::reconnect::{ConnectionFactory, ConnectionState, ReconnectingRuntime};
use nexus_bootstrap::remote::{build_remote_runtime_over_pipes, NoopTransportGuard, RemoteRuntime};
use nexus_bootstrap::{build_cli_runtime, init_forge, Runtime};
use nexus_remote::RemoteServer;
use serde_json::{json, Value};
use tokio::io::AsyncWrite;
use tokio::sync::Mutex;

/// Build a fresh `(RemoteServer, forge_guard, server_handle, runtime)`
/// quadruple. The runtime is wired to the server over a duplex pair.
fn boot_one() -> (
    RemoteRuntime,
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
    Box::leak(Box::new(kernel));

    let server =
        RemoteServer::new(Arc::new(context), event_bus).with_timeout(Duration::from_secs(30));

    let (client_writer, server_reader) = tokio::io::duplex(64 * 1024);
    let (server_writer, client_reader) = tokio::io::duplex(64 * 1024);

    let server_handle = tokio::spawn(async move {
        server
            .serve(server_reader, server_writer)
            .await
            .expect("server serve");
    });

    let writer_boxed: Box<dyn AsyncWrite + Unpin + Send> = Box::new(client_writer);
    let runtime =
        build_remote_runtime_over_pipes(client_reader, writer_boxed, Box::new(NoopTransportGuard));
    (runtime, forge, server_handle)
}

/// A factory that hands out pre-built `RemoteRuntime`s from a stack in
/// FIFO order. Used so the test can wire the reconnect path to a
/// fresh server transport when the first one is torn down.
///
/// Holds the forge / server-handle guards from each booted runtime so
/// they outlive the test rather than being dropped mid-boot.
struct StackedFactory {
    pending: Mutex<Vec<RemoteRuntime>>,
    builds: Arc<std::sync::atomic::AtomicUsize>,
    _guards: Vec<tempfile::TempDir>,
    _servers: Vec<tokio::task::JoinHandle<()>>,
}

impl ConnectionFactory for StackedFactory {
    fn build<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>> {
        Box::pin(async move {
            self.builds
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut pending = self.pending.lock().await;
            // Remove from the front so the test reads them in the order
            // they were pushed.
            if pending.is_empty() {
                Err(anyhow::anyhow!("factory exhausted"))
            } else {
                Ok(pending.remove(0))
            }
        })
    }
}

#[tokio::test]
async fn invoker_recovers_after_first_transport_dies() {
    // Build two independent server transports up front. After the
    // first runtime's `shutdown` kills its router, the reconnecting
    // invoker should pull the second runtime out of the factory and
    // retry the call.
    let (rt1, forge1, server1) = boot_one();
    let (rt2, forge2, server2) = boot_one();
    let factory = StackedFactory {
        pending: Mutex::new(vec![rt1, rt2]),
        builds: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        _guards: vec![forge1, forge2],
        _servers: vec![server1, server2],
    };
    let builds = Arc::clone(&factory.builds);
    let runtime = ReconnectingRuntime::new(Arc::new(factory))
        .with_backoff(vec![Duration::from_millis(10), Duration::from_millis(20)]);

    let invoker = runtime.invoker();

    // First call: forces the factory to mint runtime #1, succeeds.
    let v = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("first call");
    assert!(!v.is_null());
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);

    // Tear down the current transport from underneath the invoker.
    // This simulates an SSH child exiting / network drop.
    runtime.reset().await;

    // Next call: the first attempt sees a missing connection so
    // `ensure_connected` rebuilds via factory. We pre-staged runtime
    // #2 so this succeeds without engaging the backoff path.
    let v = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("post-reset call");
    assert!(!v.is_null());
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn server_error_does_not_trigger_reconnect() {
    let (rt1, forge1, server1) = boot_one();
    let factory = StackedFactory {
        pending: Mutex::new(vec![rt1]),
        builds: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        _guards: vec![forge1],
        _servers: vec![server1],
    };
    let builds = Arc::clone(&factory.builds);
    let runtime = ReconnectingRuntime::new(Arc::new(factory));
    let invoker = runtime.invoker();

    // Provoke a -32000 server error. Should surface as `Remote { code,
    // message }`, NOT trigger a reconnect (the connection is healthy;
    // the *call* failed).
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
        IpcInvokerError::Remote { code, .. } => assert_eq!(code, -32000),
        other => panic!("expected Remote, got: {other}"),
    }
    // Only the initial build — no reconnect attempts.
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn schedule_exhaustion_surfaces_final_transport_error() {
    // Build one good runtime so the first call lands, then exhaust
    // the factory. After we reset, the reconnect path will burn
    // through the backoff schedule and surface "schedule exhausted".
    let (rt1, forge1, server1) = boot_one();
    let factory = StackedFactory {
        pending: Mutex::new(vec![rt1]),
        builds: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        _guards: vec![forge1],
        _servers: vec![server1],
    };
    let builds = Arc::clone(&factory.builds);
    let runtime = ReconnectingRuntime::new(Arc::new(factory))
        .with_backoff(vec![Duration::from_millis(5), Duration::from_millis(5)]);
    let invoker = runtime.invoker();

    // First call: build #1, succeeds.
    let _ = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("first call");
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);

    // Reset; subsequent ipc_call drives ensure_connected, factory
    // returns Err for build #2, and reconnect surface is
    // "initial connection".
    runtime.reset().await;
    let err = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    match err {
        IpcInvokerError::Transport(msg) => {
            assert!(
                msg.contains("initial connection") || msg.contains("schedule exhausted"),
                "unexpected: {msg}"
            );
        }
        other => panic!("expected Transport, got: {other}"),
    }
    assert!(
        builds.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "factory should have been called more than once"
    );
}

#[tokio::test]
async fn state_transitions_emit_on_reconnect_lifecycle() {
    // Pre-stage two runtimes so reset → next call rebuilds via the
    // factory and the state subscriber can observe the full
    // Reconnecting → Connected arc.
    let (rt1, forge1, server1) = boot_one();
    let (rt2, forge2, server2) = boot_one();
    let factory = StackedFactory {
        pending: Mutex::new(vec![rt1, rt2]),
        builds: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        _guards: vec![forge1, forge2],
        _servers: vec![server1, server2],
    };
    let runtime =
        ReconnectingRuntime::new(Arc::new(factory)).with_backoff(vec![Duration::from_millis(5)]);

    let mut state_rx = runtime.subscribe_state();
    let invoker = runtime.invoker();

    // First call: Connected (the bare "successful first call" arm).
    let _ = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("first call");
    assert_eq!(
        state_rx.recv().await.expect("connected event"),
        ConnectionState::Connected
    );

    // Tear down the current transport. The next call will hit the
    // reconnect path: Reconnecting → Connected via the second pre-
    // staged runtime.
    runtime.reset().await;

    let _ = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("post-reset call");

    // Could see either Connected directly (if ensure_connected paved
    // the way before ipc_call's first attempt) OR Reconnecting →
    // Connected. Drain everything we got, asserting the final state
    // is Connected.
    let mut last: Option<ConnectionState> = None;
    while let Ok(Ok(state)) = tokio::time::timeout(Duration::from_millis(50), state_rx.recv()).await
    {
        last = Some(state);
    }
    assert_eq!(last, Some(ConnectionState::Connected));
}

#[tokio::test]
async fn schedule_exhaustion_emits_disconnected() {
    let (rt1, forge1, server1) = boot_one();
    let factory = StackedFactory {
        pending: Mutex::new(vec![rt1]),
        builds: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        _guards: vec![forge1],
        _servers: vec![server1],
    };
    let runtime = ReconnectingRuntime::new(Arc::new(factory))
        .with_backoff(vec![Duration::from_millis(5), Duration::from_millis(5)]);

    let mut state_rx = runtime.subscribe_state();
    let invoker = runtime.invoker();

    // First call succeeds → Connected.
    let _ = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("first call");

    runtime.reset().await;

    // Reset means the factory is empty; the post-reset call walks
    // the (very tight) backoff schedule, can't rebuild, surfaces
    // Disconnected.
    let _ = invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();

    let mut saw_disconnected = false;
    while let Ok(Ok(state)) = tokio::time::timeout(Duration::from_millis(50), state_rx.recv()).await
    {
        if state == ConnectionState::Disconnected {
            saw_disconnected = true;
            break;
        }
    }
    assert!(saw_disconnected, "should have observed Disconnected");
}

#[tokio::test]
async fn first_call_through_invoker_builds_lazily() {
    // The reconnecting invoker should NOT build a connection at
    // construction time — only at first dispatch.
    let factory = StackedFactory {
        pending: Mutex::new(vec![]),
        builds: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        _guards: vec![],
        _servers: vec![],
    };
    let builds = Arc::clone(&factory.builds);
    let runtime = ReconnectingRuntime::new(Arc::new(factory));
    let _invoker = runtime.invoker();
    // No dispatch yet → no factory invocation.
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 0);
}

fn _assert_value_is_value(_: &Value) {}
