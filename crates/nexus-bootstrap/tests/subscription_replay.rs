//! BL-146 — verify that subscriptions installed via
//! [`ReconnectingRuntime::subscribe`] survive a transport drop. The
//! watchdog should rebuild the underlying client and replay every
//! registered subscription so the subscriber's sink keeps receiving
//! events without manual re-subscription.
//!
//! Test fixture mirrors `reconnect_loop.rs`'s `StackedFactory` — a
//! factory that hands out pre-built `RemoteRuntime`s in FIFO order,
//! each wired to a real `RemoteServer` over a `tokio::io::duplex`
//! pair. Forcing a transport drop = aborting the server task, which
//! closes its end of the duplex and causes the client router to see
//! EOF.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use nexus_bootstrap::reconnect::{ConnectionFactory, ReconnectingRuntime};
use nexus_bootstrap::remote::{
    build_remote_runtime_over_pipes, NoopTransportGuard, RemoteRuntime,
};
use nexus_bootstrap::{build_cli_runtime, init_forge, Runtime};
use nexus_kernel::EventBus;
use nexus_remote::{EventDelivery, RemoteServer};
use serde_json::json;
use tokio::io::AsyncWrite;
use tokio::sync::{mpsc, Mutex};

/// Per-runtime quadruple — caller stores the bus so they can publish
/// matching events through it and the tempdir so it outlives the test.
struct BootedOne {
    runtime: RemoteRuntime,
    bus: Arc<EventBus>,
    _forge: tempfile::TempDir,
    _server_handle: tokio::task::JoinHandle<()>,
}

fn boot_one() -> BootedOne {
    let forge = tempfile::tempdir().expect("tempdir");
    init_forge(forge.path()).expect("init_forge");
    let local = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");
    let Runtime {
        kernel,
        context,
        loader: _loader,
    } = local;
    let bus = kernel.event_bus();
    // Leak the kernel — its lifetime needs to outlive the server, and
    // dropping it mid-test would tear down every plugin while the
    // server is still serving (matches `reconnect_loop::boot_one`).
    Box::leak(Box::new(kernel));

    let server = RemoteServer::new(Arc::new(context), Arc::clone(&bus))
        .with_timeout(Duration::from_secs(30));

    let (client_writer, server_reader) = tokio::io::duplex(64 * 1024);
    let (server_writer, client_reader) = tokio::io::duplex(64 * 1024);

    let server_handle = tokio::spawn(async move {
        let _ = server.serve(server_reader, server_writer).await;
    });

    let writer_boxed: Box<dyn AsyncWrite + Unpin + Send> = Box::new(client_writer);
    let runtime = build_remote_runtime_over_pipes(
        client_reader,
        writer_boxed,
        Box::new(NoopTransportGuard),
    );
    BootedOne {
        runtime,
        bus,
        _forge: forge,
        _server_handle: server_handle,
    }
}

/// Factory that hands out pre-built `RemoteRuntime`s in FIFO order +
/// keeps the server JoinHandles around so the caller can abort them
/// to simulate a transport drop.
struct StackedFactory {
    pending: Mutex<Vec<RemoteRuntime>>,
    _guards: Vec<tempfile::TempDir>,
    /// Kept around so the server task isn't dropped (and its duplex
    /// halves closed) before the factory hands the corresponding
    /// `RemoteRuntime` to the reconnect wrapper.
    _server_handles: Vec<tokio::task::JoinHandle<()>>,
    builds: Arc<AtomicUsize>,
}

impl ConnectionFactory for StackedFactory {
    fn build<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>> {
        Box::pin(async move {
            self.builds
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut pending = self.pending.lock().await;
            if pending.is_empty() {
                Err(anyhow::anyhow!("factory exhausted"))
            } else {
                Ok(pending.remove(0))
            }
        })
    }
}

/// Two-runtime fixture: subscribe → kill the first transport → verify
/// the next publish (on the *second* server's bus) reaches the
/// subscriber's sink without the caller re-issuing `subscribe`.
#[tokio::test]
async fn subscription_replays_after_transport_drop() {
    let booted1 = boot_one();
    let booted2 = boot_one();

    let bus1: Arc<EventBus> = Arc::clone(&booted1.bus);
    let bus2: Arc<EventBus> = Arc::clone(&booted2.bus);

    let factory = StackedFactory {
        pending: Mutex::new(vec![booted1.runtime, booted2.runtime]),
        _guards: vec![booted1._forge, booted2._forge],
        _server_handles: vec![booted1._server_handle, booted2._server_handle],
        builds: Arc::new(AtomicUsize::new(0)),
    };
    let runtime = ReconnectingRuntime::new(Arc::new(factory)).with_backoff(vec![
        Duration::from_millis(20),
        Duration::from_millis(40),
    ]);
    let mut replay_rx = runtime.subscribe_replays();

    // Establish the initial connection with an ipc_call. This fires
    // install_runtime_under_lock which sends a replay event with N=0
    // (registry empty at this point).
    let invoker = runtime.invoker();
    invoker
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            json!({ "path": "" }),
            Duration::from_secs(5),
        )
        .await
        .expect("initial ipc_call");
    // Drain the initial-install replay event (N=0).
    let _ = tokio::time::timeout(Duration::from_millis(200), replay_rx.recv()).await;

    // Now subscribe against the live runtime #1.
    let (tx, mut rx) = mpsc::unbounded_channel::<EventDelivery>();
    runtime
        .subscribe(
            "test-sub",
            json!({ "kind": "custom_prefix", "prefix": "com.nexus.storage." }),
            tx,
        )
        .await
        .expect("initial subscribe");

    // Sanity check — publish on bus1 reaches the sink while connected
    // to runtime #1.
    bus1.publish_plugin(
        "com.nexus.storage",
        "com.nexus.storage.test_event",
        json!({ "phase": "pre-drop" }),
    )
    .expect("publish pre-drop");
    let pre = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("pre-drop event timed out");
    assert!(pre.is_some(), "pre-drop receiver closed");

    // Force a transport drop by shutting down the current client
    // directly — equivalent to the server's stdout half EOF'ing.
    // `wait_for_disconnect` fires inside the watchdog, which then
    // walks the backoff schedule and rebuilds against runtime #2.
    let current_client = runtime.ensure_client().await.expect("ensure_client #1");
    current_client.shutdown().await;

    // Watchdog should detect the drop, walk the backoff, build
    // runtime #2, replay the subscription, then publish on bus2 should
    // reach the sink.
    //
    // Wait for the replay notification confirming a non-zero replay
    // happened.
    let mut saw_replay = false;
    for _ in 0..10 {
        if let Ok(Ok(n)) =
            tokio::time::timeout(Duration::from_secs(2), replay_rx.recv()).await
        {
            if n >= 1 {
                saw_replay = true;
                break;
            }
        }
    }
    assert!(
        saw_replay,
        "expected a replay notification with N>=1 after transport drop"
    );

    // Now publish on the SECOND server's bus and verify the sink
    // receives it.
    bus2.publish_plugin(
        "com.nexus.storage",
        "com.nexus.storage.test_event",
        json!({ "phase": "post-replay" }),
    )
    .expect("publish post-replay");
    let post = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("post-replay event timed out");
    assert!(
        post.is_some(),
        "subscriber sink should receive event after replay"
    );
}

/// `subscribe` issued while no client is currently connected queues
/// the entry in the registry; the next install (triggered by an
/// `ensure_client` or `ipc_call`) replays it.
#[tokio::test]
async fn subscribe_queues_when_disconnected_and_installs_on_next_connect() {
    let booted = boot_one();
    let bus = Arc::clone(&booted.bus);
    let factory = StackedFactory {
        pending: Mutex::new(vec![booted.runtime]),
        _guards: vec![booted._forge],
        _server_handles: vec![booted._server_handle],
        builds: Arc::new(AtomicUsize::new(0)),
    };
    let runtime = ReconnectingRuntime::new(Arc::new(factory)).with_backoff(vec![
        Duration::from_millis(20),
    ]);

    // No connection yet — subscribe should just queue the entry.
    let (tx, mut rx) = mpsc::unbounded_channel::<EventDelivery>();
    runtime
        .subscribe(
            "queued-sub",
            json!({ "kind": "custom_prefix", "prefix": "com.nexus.storage." }),
            tx,
        )
        .await
        .expect("subscribe while disconnected");

    // Force a connection by calling ensure_client. The install path
    // runs install_runtime_under_lock, which replays the queued
    // subscription against the new client.
    let _ = runtime
        .ensure_client()
        .await
        .expect("ensure_client triggers install");

    // Wait briefly for replay to settle, then publish + verify.
    tokio::time::sleep(Duration::from_millis(50)).await;
    bus.publish_plugin(
        "com.nexus.storage",
        "com.nexus.storage.test_event",
        json!({ "x": 1 }),
    )
    .expect("publish");
    let delivery = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("event delivery timed out");
    assert!(
        delivery.is_some(),
        "queued subscription should have been installed on first connect"
    );
}

/// `unsubscribe` removes the entry from the registry so it does NOT
/// get replayed on the next reconnect.
#[tokio::test]
async fn unsubscribe_clears_registry_so_replay_is_skipped() {
    let booted1 = boot_one();
    let booted2 = boot_one();
    let bus2: Arc<EventBus> = Arc::clone(&booted2.bus);

    let factory = StackedFactory {
        pending: Mutex::new(vec![booted1.runtime, booted2.runtime]),
        _guards: vec![booted1._forge, booted2._forge],
        _server_handles: vec![booted1._server_handle, booted2._server_handle],
        builds: Arc::new(AtomicUsize::new(0)),
    };
    let runtime = ReconnectingRuntime::new(Arc::new(factory)).with_backoff(vec![
        Duration::from_millis(20),
    ]);
    let mut replay_rx = runtime.subscribe_replays();

    let (tx, mut rx) = mpsc::unbounded_channel::<EventDelivery>();
    runtime
        .subscribe(
            "soon-cancelled",
            json!({ "kind": "custom_prefix", "prefix": "com.nexus.storage." }),
            tx,
        )
        .await
        .expect("initial subscribe");
    // Drain the initial-install replay.
    let _ = tokio::time::timeout(Duration::from_millis(200), replay_rx.recv()).await;

    // Cancel the sub before triggering a reconnect.
    let _ = runtime.unsubscribe("soon-cancelled").await;

    // Force a transport drop.
    let current_client = runtime.ensure_client().await.expect("ensure_client");
    current_client.shutdown().await;

    // The watchdog will reconnect against runtime #2 and emit a
    // replay event — but since the registry is empty, the count
    // should be 0.
    let mut saw_zero = false;
    for _ in 0..10 {
        if let Ok(Ok(n)) =
            tokio::time::timeout(Duration::from_secs(2), replay_rx.recv()).await
        {
            if n == 0 {
                saw_zero = true;
                break;
            }
        }
    }
    assert!(
        saw_zero,
        "expected replay notification with N=0 after unsubscribe"
    );

    // Publish on bus2 — the sink should NOT receive (the sub was
    // cancelled before reconnect).
    bus2.publish_plugin(
        "com.nexus.storage",
        "com.nexus.storage.test_event",
        json!({ "should": "not_arrive" }),
    )
    .expect("publish");
    let arrived = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    assert!(
        arrived.is_err() || matches!(arrived, Ok(None)),
        "cancelled subscription should not receive replayed events"
    );
}
