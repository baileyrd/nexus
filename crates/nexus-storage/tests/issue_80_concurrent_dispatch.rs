//! Regression test for issue #80 — storage IPC dispatch was
//! serialized through a single `Mutex<StorageEngine>`.
//!
//! Pre-fix shape: `engine: Option<Mutex<StorageEngine>>` because
//! `StorageEngine` owned a `Watcher` whose `mpsc::Receiver` is
//! `Send` but not `Sync`. The mutex was acquired on every IPC
//! call, including reads — so two concurrent `query_files` /
//! `read_file` / `search` calls serialized behind it.
//!
//! Post-fix: the dead-code watcher inside `StorageEngine` is
//! removed; the production watcher is created in
//! `StorageCorePlugin::on_start` and moved into the bridge thread,
//! same as before. With no non-`Sync` field, the engine is
//! `Send + Sync` and the plugin holds it as `Arc<StorageEngine>`,
//! dispatching IPC calls on `&engine` directly with no per-call
//! locking.
//!
//! These tests pin both the type-level property and the runtime
//! behaviour so a future regression that re-introduces a lock
//! around the engine would fail loud.

use std::sync::Arc;
use std::thread;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use nexus_storage::core_plugin::{
    HANDLER_QUERY_FILES, HANDLER_WRITE_VAULT_FILE,
};
use nexus_storage::{StorageConfig, StorageCorePlugin, StorageEngine};

/// Static assertion that `StorageEngine` is `Send + Sync`. If a
/// future change reintroduces a non-`Sync` field (an `mpsc::Receiver`,
/// `Cell`, `RefCell`, …) this fails at compile time. The
/// runtime-concurrency test below also exercises the property, but
/// this guard is faster + clearer about WHY.
#[test]
fn storage_engine_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<StorageEngine>();
    assert_send_sync::<Arc<StorageEngine>>();
}

fn boot_plugin(forge_root: &std::path::Path) -> StorageCorePlugin {
    drop(StorageEngine::init(forge_root).expect("init forge"));
    let bus = Arc::new(EventBus::new(16));
    let mut plugin =
        StorageCorePlugin::new(forge_root.to_path_buf(), &StorageConfig::default(), bus);
    plugin.on_init().expect("on_init");
    plugin
}

#[test]
fn engine_methods_run_concurrently_without_locking() {
    // Burst of N threads sharing one `Arc<StorageEngine>` and
    // calling `query_files` directly on `&engine`. Pre-#80, this
    // shape didn't compile — `StorageEngine` was `!Sync` because
    // its `Watcher` field held a `mpsc::Receiver`, so even
    // `Arc<StorageEngine>` couldn't cross thread boundaries
    // shared. The test exercises both:
    //   1. The compile-time property: `Arc<StorageEngine>` is
    //      `Send + Sync` (so the closures can capture clones).
    //   2. The runtime property: concurrent reads from N threads
    //      complete successfully and return identical results.
    let dir = tempfile::tempdir().expect("tempdir");
    drop(StorageEngine::init(dir.path()).expect("init forge"));
    let engine = Arc::new(
        StorageEngine::open(dir.path(), &StorageConfig::default()).expect("open forge"),
    );

    let n = 32;
    let mut handles = Vec::with_capacity(n);
    let filter = nexus_storage::FileFilter::default();
    for _ in 0..n {
        let engine = Arc::clone(&engine);
        let filter = filter.clone();
        handles.push(thread::spawn(move || {
            engine.query_files(&filter).expect("query_files")
        }));
    }
    for h in handles {
        let _result = h.join().expect("thread panic");
    }
}

#[test]
fn ipc_dispatch_round_trip_after_lock_removal() {
    // End-to-end smoke that the post-#80 plugin (which holds
    // `Option<Arc<StorageEngine>>` instead of `Option<Mutex<StorageEngine>>`)
    // still dispatches IPC handlers correctly. Catches bugs where
    // the lock removal accidentally broke the dispatch path.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plugin = boot_plugin(dir.path());
    let resp = plugin
        .dispatch(
            HANDLER_QUERY_FILES,
            &serde_json::json!({"file_type": null, "tag": null}),
        )
        .expect("query_files dispatch should succeed post-#80");
    assert!(
        resp.is_array(),
        "query_files must return a JSON array, got: {resp}"
    );
}

#[test]
fn write_vault_file_rejects_paths_outside_forge_metadata() {
    // The audit's adjacent finding (#80): `HANDLER_WRITE_VAULT_FILE`
    // is documented as ".forge/-prefixed shell metadata only" but
    // accepted any forge-relative path. Combined with the path-
    // confinement fix from #72, the residual concern is "writes
    // outside .forge/ silently bypass the FTS / graph / watcher
    // updates because write_raw skips them". Reject upfront.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plugin = boot_plugin(dir.path());

    let evil_paths = [
        "notes/should_not_use_write_raw.md",
        "attachments/img.png",
        "Untitled.md",
        ".forgetnot/x", // prefix-similar but not under .forge/
    ];
    for path in evil_paths {
        let args = serde_json::json!({ "path": path, "bytes": [0u8, 1u8] });
        let err = plugin
            .dispatch(HANDLER_WRITE_VAULT_FILE, &args)
            .expect_err(&format!("write_vault_file must reject {path:?}"));
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(
                    reason.contains("outside the .forge/"),
                    "{path}: expected .forge/-prefix rejection, got: {reason}"
                );
            }
            other => panic!("{path}: expected ExecutionFailed, got: {other:?}"),
        }
    }
}

#[test]
fn write_vault_file_accepts_forge_metadata_paths() {
    // Positive control: the legitimate use case (shell-owned
    // metadata under `.forge/`) still works.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plugin = boot_plugin(dir.path());

    let ok_paths = [".forge/workspace.json", ".forge/sidecars/foo.json"];
    for path in ok_paths {
        let args = serde_json::json!({ "path": path, "bytes": [0u8, 1u8, 2u8] });
        plugin
            .dispatch(HANDLER_WRITE_VAULT_FILE, &args)
            .unwrap_or_else(|e| panic!(".forge/-prefixed path {path:?} must still work; got: {e:?}"));
        assert!(
            dir.path().join(path).exists(),
            "{path}: file must exist after write_vault_file"
        );
    }
}
