//! Internal tests for [`super::KernelPluginContext`] (V11,
//! `repo-review-2026-06-10.md` — lifted out of `context_impl.rs`,
//! mirroring the #191 nexus-storage test split). Declared as a child
//! module via `#[path]`, so `use super::*` retains access to the
//! parent module's private items.

use super::dispatch::spawn_blocking_sync_dispatch;
use super::*;
use crate::event::NexusEvent;
use crate::kv_store::InMemoryKvStore;

fn make_context(dir: &Path, caps: &[Capability]) -> KernelPluginContext {
    let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
    let bus = Arc::new(EventBus::new(16));
    KernelPluginContext::new(
        "com.test.plugin",
        "1.0.0",
        caps.iter().copied().collect::<CapabilitySet>(),
        kv,
        bus,
        dir,
        None,
    )
    .unwrap()
}

#[test]
fn identity_methods_return_correct_values() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[]);
    assert_eq!(ctx.plugin_id(), "com.test.plugin");
    assert_eq!(ctx.plugin_version(), "1.0.0");
}

#[test]
fn has_capability_reflects_granted_caps() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::KvRead]);
    assert!(ctx.has_capability(Capability::KvRead));
    assert!(!ctx.has_capability(Capability::KvWrite));
}

#[test]
fn caps_recover_from_poisoned_lock() {
    // #199 / R16 — poisoning the capabilities `RwLock` (by panicking
    // inside a write-side closure) must not propagate to readers.
    // Both `capabilities_snapshot` and the private `caps_contains`
    // (exercised here via `has_capability`) recover with
    // `PoisonError::into_inner` and continue serving the inner
    // state. Without the recovery, every subsequent read would
    // `.expect()`-panic; under `panic = "abort"` that would abort
    // the whole runtime over an unrelated subsystem failure.
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::KvRead]);
    let caps_handle = ctx.caps_handle();

    // Poison the RwLock by panicking while holding the write guard.
    let caps_for_panic = Arc::clone(&caps_handle);
    let _ = std::thread::spawn(move || {
        let _guard = caps_for_panic.write().unwrap();
        panic!("intentional poison for test");
    })
    .join();

    // Sanity: the lock is now poisoned.
    assert!(caps_handle.read().is_err(), "lock should be poisoned");

    // The recovery paths still serve the original cap.
    assert!(
        ctx.has_capability(Capability::KvRead),
        "has_capability must recover from a poisoned RwLock",
    );
    assert!(
        !ctx.has_capability(Capability::KvWrite),
        "recovery must not invent caps the plugin doesn't hold",
    );
    let snapshot = ctx.capabilities_snapshot();
    assert!(snapshot.contains(Capability::KvRead));
    assert!(!snapshot.contains(Capability::KvWrite));
}

#[tokio::test]
async fn kv_requires_capability() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[]);
    assert!(ctx.kv_get("key").await.is_err());
    assert!(ctx.kv_set("key", b"val").await.is_err());
    assert!(ctx.kv_list("").await.is_err());
}

#[tokio::test]
async fn kv_get_set_delete_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::KvRead, Capability::KvWrite]);
    ctx.kv_set("key", b"hello").await.unwrap();
    let val = ctx.kv_get("key").await.unwrap().unwrap();
    assert_eq!(val, b"hello");
    ctx.kv_delete("key").await.unwrap();
    assert!(ctx.kv_get("key").await.unwrap().is_none());
}

#[tokio::test]
async fn kv_list_requires_only_read_capability_and_filters_by_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::KvRead, Capability::KvWrite]);
    ctx.kv_set("settings.theme", b"a").await.unwrap();
    ctx.kv_set("settings.font", b"b").await.unwrap();
    ctx.kv_set("cache.foo", b"c").await.unwrap();

    let mut keys = ctx.kv_list("settings.").await.unwrap();
    keys.sort();
    assert_eq!(keys, vec!["settings.font", "settings.theme"]);

    let read_only = make_context(dir.path(), &[Capability::KvRead]);
    assert!(read_only.kv_list("").await.is_ok());
    assert!(read_only.kv_set("x", b"y").await.is_err());
}

#[test]
fn publish_rejects_namespace_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[]);
    let result = ctx.publish("com.other.event", serde_json::json!({}));
    assert!(result.is_err());
}

/// Regression for issue #79. The pre-fix check at the context
/// boundary was `type_id.starts_with(plugin_id)`, allowing a plugin
/// with id `com.test` to publish topics namespaced under
/// `com.testimony` etc. `make_context` uses `com.test.plugin`, so
/// the substring-prefix attack here is `com.test.plugin*` → the
/// hostile `com.test.plugin-evil.event` would have passed pre-fix.
#[test]
fn publish_rejects_substring_prefix_spoof() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[]);
    // Same-prefix-different-namespace: shares the `com.test.plugin`
    // characters but `-evil` breaks the dotted boundary, so the
    // strict check rejects it.
    let result = ctx.publish("com.test.plugin-evil.event", serde_json::json!({}));
    assert!(
        result.is_err(),
        "com.test.plugin must NOT be allowed to publish com.test.plugin-evil.event",
    );
}

#[test]
fn publish_allows_dotted_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[]);
    ctx.publish("com.test.plugin.event", serde_json::json!({}))
        .expect("dotted suffix is the legitimate namespace shape");
}

#[tokio::test]
async fn publish_emits_to_subscriber() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[]);
    let mut sub = ctx.subscribe(EventFilter::All);

    ctx.publish("com.test.plugin.ping", serde_json::json!({"x": 1}))
        .unwrap();

    let evt = sub.recv().await.unwrap();
    match &evt.event {
        NexusEvent::Custom { type_id, .. } => assert_eq!(type_id, "com.test.plugin.ping"),
        _ => panic!("wrong event"),
    }
}

#[tokio::test]
async fn read_file_denied_without_capability() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[]);
    let result = ctx.read_file(&dir.path().join("test.txt")).await;
    assert!(result.is_err());
}

/// Coverage for OI-07: a denied capability gate routes through
/// `audit::log_capability_denied`, not an ad-hoc `tracing::warn!`. Asserts
/// the structured `audit=true` field reaches the tracing channel so a
/// security-stream filter can pick it up.
#[test]
fn capability_denial_emits_audit_event_through_gate() {
    let events = audit::test_support::with_captured_events_async(|| async {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        let _ = ctx.kv_get("anything").await;
    });
    let denial = events
        .iter()
        .find(|e| e.contains("audit=true") && e.contains("result=denied"))
        .unwrap_or_else(|| panic!("no audit denial event emitted; got: {events:?}"));
    assert!(denial.contains("plugin_id=com.test.plugin"), "{denial}");
    assert!(denial.contains("capability=kv.read"), "{denial}");
}

/// Coverage for OI-07: a path-traversal rejection routes through
/// `audit::log_path_traversal_denied` and reaches the structured channel.
#[test]
fn path_traversal_emits_audit_event_through_gate() {
    let events = audit::test_support::with_captured_events_async(|| async {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::FsRead]);
        let _ = ctx.read_file(Path::new("/etc/passwd")).await;
    });
    let traversal = events
        .iter()
        .find(|e| e.contains("audit=true") && e.contains("path traversal denied"))
        .unwrap_or_else(|| panic!("no audit traversal event emitted; got: {events:?}"));
    assert!(
        traversal.contains("plugin_id=com.test.plugin"),
        "{traversal}"
    );
}

#[tokio::test]
async fn read_write_file_with_capability() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::FsRead, Capability::FsWrite]);
    let file_path = dir.path().join("test.txt");
    ctx.write_file(&file_path, b"hello forge").await.unwrap();
    let contents = ctx.read_file(&file_path).await.unwrap();
    assert_eq!(contents, b"hello forge");
}

#[tokio::test]
async fn confine_path_blocks_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::FsRead]);
    // Try to read /etc/passwd via traversal
    let result = ctx.read_file(Path::new("/etc/passwd")).await;
    assert!(result.is_err());
}

/// OI-12 acceptance: an absolute path outside the forge must produce
/// a *loud, typed* failure, not a silent denial — no auto-promotion
/// from `FsRead` to `FsReadExternal`. We assert the error is the
/// `PermissionDenied` traversal variant (not, say, a generic
/// `CapabilityDenied`) so callers can distinguish "you asked for a
/// file outside the forge" from "you don't hold `FsRead` at all".
#[tokio::test]
async fn read_file_absolute_outside_forge_returns_typed_traversal_error() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::FsRead]);
    let err = ctx
        .read_file(Path::new("/etc/passwd"))
        .await
        .expect_err("absolute outside-forge read must fail");
    match err {
        Error::Io(io_err) => {
            assert_eq!(
                io_err.kind(),
                std::io::ErrorKind::PermissionDenied,
                "expected PermissionDenied, got {:?}",
                io_err.kind(),
            );
            assert!(
                io_err.to_string().contains("path traversal denied"),
                "expected traversal message, got: {io_err}",
            );
        }
        other => panic!("expected Error::Io, got {other:?}"),
    }
}

/// OI-12 mirror for the write side. `validate_for_write` strips the
/// leading `/` and treats absolute inputs as forge-root-relative; an
/// absolute path that resolves outside the forge (here via a `..`
/// payload) hits the same `PermissionDenied` traversal path.
#[tokio::test]
async fn write_file_absolute_outside_forge_returns_typed_traversal_error() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = make_context(dir.path(), &[Capability::FsWrite]);
    let err = ctx
        .write_file(Path::new("/../escape.txt"), b"x")
        .await
        .expect_err("absolute traversal write must fail");
    match err {
        Error::Io(io_err) => {
            assert_eq!(
                io_err.kind(),
                std::io::ErrorKind::PermissionDenied,
                "expected PermissionDenied, got {:?}",
                io_err.kind(),
            );
            assert!(
                io_err.to_string().contains("path traversal denied"),
                "expected traversal message, got: {io_err}",
            );
        }
        other => panic!("expected Error::Io, got {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn write_file_rejects_symlinked_parent() {
    // Regression for MK F-5.3.2: a symlinked parent directory must not
    // let a plugin write outside the forge root. `validate_for_write`
    // canonicalizes the deepest existing ancestor (the symlink target)
    // and the prefix check rejects it.
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();

    let ctx = make_context(dir.path(), &[Capability::FsWrite]);
    let result = ctx
        .write_file(&dir.path().join("escape/victim.txt"), b"pwned")
        .await;
    assert!(result.is_err(), "write through symlinked parent must fail");
    // The file must not have been created outside the sandbox.
    assert!(!outside.path().join("victim.txt").exists());
}

// ── Sync dispatch blocking-pool counter ──────────────────────────────────
//
// The counter is process-global static state. We can't reliably observe a
// specific peak without serialising the test (other tests in this file
// don't fire IPC, so the counter is effectively private to this test in
// practice, but we still capture a baseline and only assert deltas).

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawn_blocking_sync_dispatch_counts_in_flight() {
    let baseline = in_flight_sync_dispatches();

    // Channel pair lets the spawned task hold the slot until the test
    // releases it — gives us a deterministic point at which to read the
    // counter mid-flight.
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let join = spawn_blocking_sync_dispatch(move || {
        // Block until the test signals release.
        let _ = release_rx.recv();
        42_u64
    });

    // Wait briefly for the blocking task to start running. Spinning on
    // the counter is more deterministic than a fixed sleep.
    let start = std::time::Instant::now();
    loop {
        if in_flight_sync_dispatches() > baseline {
            break;
        }
        if start.elapsed() > Duration::from_secs(2) {
            panic!(
                "counter never incremented; baseline={baseline}, \
                 observed={}",
                in_flight_sync_dispatches()
            );
        }
        tokio::task::yield_now().await;
    }

    let mid = in_flight_sync_dispatches();
    assert!(
        mid > baseline,
        "expected in-flight count to rise by at least 1; baseline={baseline}, mid={mid}",
    );

    // Release and let the spawned task finish; counter must return to
    // (or below — other tests may have decremented) the baseline.
    release_tx.send(()).expect("release");
    let result = join.await.expect("join");
    assert_eq!(result, 42);

    let post = in_flight_sync_dispatches();
    assert!(
        post < mid,
        "expected at least one decrement after task completion; \
         mid={mid}, post={post}",
    );
}

// ── Track A: cooperative cancellation through ipc_call ───────────────────

/// Async-only dispatcher whose `dispatch_async` future sleeps for 10 s.
/// Used to verify the cancel race short-circuits long-running calls.
struct SlowAsyncDispatcher;

impl crate::ipc::IpcDispatcher for SlowAsyncDispatcher {
    fn dispatch(
        &self,
        _target_plugin_id: &str,
        _command_id: &str,
        _args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, IpcError> {
        unreachable!("test routes through dispatch_async only");
    }

    fn dispatch_async(
        &self,
        _target_plugin_id: &str,
        _command_id: &str,
        _args: serde_json::Value,
    ) -> Option<crate::ipc::IpcFuture> {
        Some(Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(serde_json::json!({"done": true}))
        }))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_call_returns_cancelled_when_parent_token_fires() {
    use crate::context::Ipc as _;
    use tokio_util::sync::CancellationToken;

    let dir = tempfile::tempdir().unwrap();
    let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
    let bus = Arc::new(EventBus::new(16));
    let dispatcher: Arc<dyn crate::ipc::IpcDispatcher> = Arc::new(SlowAsyncDispatcher);
    let ctx = KernelPluginContext::new(
        "com.test.caller",
        "1.0.0",
        [Capability::IpcCall].into_iter().collect::<CapabilitySet>(),
        kv,
        bus,
        dir.path(),
        Some(dispatcher),
    )
    .unwrap();

    let parent = CancellationToken::new();
    let to_fire = parent.clone();
    // Trip the parent token after a short delay so the in-flight call
    // observes it via the child-token derived inside ipc_call_inner.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        to_fire.cancel();
    });

    let started = std::time::Instant::now();
    let result = crate::cancel::scope_async(parent, async {
        ctx.ipc_call(
            "com.target",
            "do",
            serde_json::json!({}),
            Duration::from_secs(10),
        )
        .await
    })
    .await;
    let elapsed = started.elapsed();

    assert!(
        matches!(result, Err(IpcError::Cancelled { .. })),
        "expected Err(IpcError::Cancelled), got {result:?}",
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "cancel must short-circuit the 10-s sleep; took {elapsed:?}",
    );
}
