//! Phase 1 acceptance gap #5 — backend smoke test.
//!
//! Proves the backend half of the "boot empty → load hello-world →
//! invoke `com.nexus.storage::list_dir` → receive kernel bus event →
//! shut down cleanly" Phase 1 §2 #5 acceptance criterion.
//!
//! Per `docs/planning/PHASE-1-IMPLEMENTATION-PLAN.md` §11 question 2, the full Tauri
//! window E2E (the "mount as View via Leaf" half) is deferred to Phase 4
//! polish. Phase 1 acceptance is satisfied by a unit-test harness on each
//! side of the bridge:
//!
//!   - Backend (this file): kernel boots → storage IPC handles a real call
//!     → events flow to a subscriber → `kernel.shutdown()` returns Ok.
//!   - Frontend (`shell/tests/subscription-cleanup.test.ts`): the matching
//!     TS-side disposer / unregister contract that lets a plugin like
//!     hello-world mount and unmount without leaking subscriptions.
//!
//! Patterned after `forge_ipc.rs` (storage IPC harness) and `theme_ipc.rs`
//! (bus-event harness).

use std::time::Duration;

use nexus_bootstrap::{build_cli_runtime, init_forge};
use nexus_kernel::{EventFilter, Ipc as _, NexusEvent};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";

#[tokio::test]
async fn phase1_smoke_boot_invoke_storage_event_shutdown() {
    // 1. Boot empty: `init_forge` creates a fresh forge with `notes/`,
    //    `attachments/`, and `.forge/` directories. `build_cli_runtime`
    //    assembles the kernel + every in-tree core plugin, including
    //    `com.nexus.storage`.
    let forge = tempfile::tempdir().expect("tempdir");
    init_forge(forge.path()).expect("init empty forge");
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    // 2. Subscribe to the storage event prefix BEFORE invoking, so the
    //    broadcast event we publish below cannot race past us.
    let mut sub = runtime
        .kernel
        .event_bus()
        .subscribe(EventFilter::CustomPrefix("com.nexus.storage.".to_string()));

    // 3. Invoke `com.nexus.storage::list_dir` on the empty forge root.
    //    `init_forge` plants `notes/` and `attachments/` (per
    //    StorageEngine::init); `.forge/` is filtered out by list_dir
    //    (forge_ipc.rs::list_dir_returns_sorted_entries_and_hides_forge_dir
    //    documents this contract).
    let entries = runtime
        .context
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "list_dir",
            serde_json::json!({ "relpath": "" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("list_dir on empty forge must succeed");

    let arr = entries.as_array().expect("list_dir returns array");
    let names: Vec<String> = arr
        .iter()
        .map(|e| e["name"].as_str().expect("entry has name").to_string())
        .collect();
    assert!(
        names.iter().any(|n| n == "notes"),
        "empty forge must contain 'notes/' dir; got {names:?}",
    );
    assert!(
        !names.iter().any(|n| n == ".forge"),
        "list_dir must hide .forge; got {names:?}",
    );

    // 4. Trigger a storage mutation and assert a `com.nexus.storage.*`
    //    event reaches the subscriber. `write_file` on a fresh path
    //    publishes `com.nexus.storage.file_created` (see core_plugin.rs
    //    publish_event for FileCreated).
    runtime
        .context
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "write_file",
            serde_json::json!({
                "path": "notes/smoke.md",
                "bytes": b"# smoke\n".to_vec(),
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect("write_file must succeed");

    // The bus is a tokio broadcast channel and storage event publishing
    // hops through a forwarder thread — give it a tick to deliver.
    let mut got_event = false;
    for _ in 0..20 {
        match sub.try_recv() {
            Ok(Some(published)) => {
                if let NexusEvent::Custom {
                    type_id, payload, ..
                } = &published.event
                {
                    if type_id.starts_with("com.nexus.storage.") {
                        // Sanity-check the payload shape so a future refactor
                        // that changes the event surface is forced through
                        // this gate.
                        assert!(
                            payload.is_object(),
                            "storage event payload must be a JSON object",
                        );
                        got_event = true;
                        break;
                    }
                }
            }
            Ok(None) | Err(_) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    assert!(
        got_event,
        "no com.nexus.storage.* event observed after write_file",
    );

    // 5. Shut down cleanly. This drives PluginManager::shutdown_all under
    //    the hood; any panic or hang here is a Phase 1 regression.
    runtime
        .kernel
        .shutdown()
        .await
        .expect("kernel.shutdown must return Ok(())");
}
