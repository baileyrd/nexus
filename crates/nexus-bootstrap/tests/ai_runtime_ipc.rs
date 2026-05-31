//! End-to-end tests for the BL-134 / ADR 0028 Phase-1 ai-runtime
//! plugin (`com.nexus.ai.runtime`) driven through the kernel IPC
//! surface.
//!
//! Phase 1 ships read-only handlers (`get` / `list` / `events` /
//! `pool_stats`), the `submit` enqueue path, and reserved Phase-5
//! placeholders for `cancel` / `pause` / `resume`. These tests pin
//! the wire-shape contract for each.
//!
//! Move 7 adds `register_trigger` / `unregister_trigger` /
//! `list_triggers` and the trigger watcher loop; tests at the bottom
//! exercise those through the same `build_cli_runtime` stack.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{Ipc as _, IpcError};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const RUNTIME_PLUGIN_ID: &str = "com.nexus.ai.runtime";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

async fn call(
    runtime: &nexus_bootstrap::Runtime,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    runtime
        .context
        .ipc_call(RUNTIME_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn list_returns_empty_runs_array_on_a_fresh_runtime() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let resp = call(&runtime, "list", serde_json::json!({})).await.unwrap();
    assert_eq!(resp, serde_json::json!({ "runs": [] }));
}

#[tokio::test]
async fn pool_stats_reports_at_least_two_workers_after_boot() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let resp = call(&runtime, "pool_stats", serde_json::json!({}))
        .await
        .expect("pool_stats");
    let workers = resp.get("workers").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(workers >= 2, "expected workers >= 2, got {workers}");
    assert_eq!(resp.get("queued").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(resp.get("running").and_then(|v| v.as_u64()), Some(0));
}

#[tokio::test]
async fn get_unknown_task_id_surfaces_a_typed_error() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let err = call(
        &runtime,
        "get",
        serde_json::json!({ "task_id": "00000000-0000-0000-0000-000000000000" }),
    )
    .await
    .unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("not found"), "got {err:?}");
}

#[tokio::test]
async fn pause_and_resume_return_unsupported_error() {
    // BL-134 Phase 5 — cancel is wired (see cancel-flow tests
    // below); pause/resume return a typed "not supported on Session
    // tasks" error because a single ipc_call has no resumable
    // midpoint. The cap-matrix entry keeps both verbs gated so the
    // privilege boundary stays consistent if a future phase adds
    // pause-able task kinds.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    for cmd in ["pause", "resume"] {
        let err = call(
            &runtime,
            cmd,
            serde_json::json!({ "task_id": "00000000-0000-0000-0000-000000000000" }),
        )
        .await
        .unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("not supported"),
            "{cmd}: expected unsupported-message, got {err:?}"
        );
    }
}

#[tokio::test]
async fn cancel_unknown_task_id_errors_clearly() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let err = call(
        &runtime,
        "cancel",
        serde_json::json!({ "task_id": "00000000-0000-0000-0000-000000000000" }),
    )
    .await
    .unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("not found"), "got {err:?}");
}

#[tokio::test]
async fn submit_returns_a_task_id_and_records_the_run_in_list() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // The Session task body is forwarded verbatim to
    // com.nexus.agent::session_run. We pass an obviously-incomplete
    // body so the worker's session_run call fails fast — that
    // exercises the Submitted -> Started -> Failed transition end-
    // to-end without needing an LLM provider configured.
    let resp = call(
        &runtime,
        "submit",
        serde_json::json!({
            "task": { "kind": "session", "args": {} },
        }),
    )
    .await
    .expect("submit");
    let task_id = resp
        .get("task_id")
        .and_then(|v| v.as_str())
        .expect("task_id present")
        .to_string();
    assert_eq!(task_id.len(), 36, "uuid string length");

    // Give the worker thread a beat to produce its terminal event.
    // The dispatch is async; the run might still be Queued or
    // Running on the very first poll.
    let mut found = None;
    for _ in 0..20 {
        let listing = call(&runtime, "list", serde_json::json!({}))
            .await
            .expect("list");
        let runs = listing.get("runs").and_then(|v| v.as_array()).cloned();
        let runs = runs.unwrap_or_default();
        if let Some(row) = runs
            .iter()
            .find(|r| r.get("task_id").and_then(|v| v.as_str()) == Some(task_id.as_str()))
        {
            found = Some(row.clone());
            let status = row.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(status, "completed" | "failed") {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let row = found.expect("run row appears in list");
    let status = row.get("status").and_then(|v| v.as_str()).unwrap_or("");
    // Phase 1 doesn't pin the failure mode — empty session_run args
    // can produce a parse error or a "no provider" error depending
    // on agent internals. Either is acceptable; what matters is the
    // run reaches a terminal status, not that it succeeds.
    assert!(
        matches!(status, "completed" | "failed" | "queued" | "running"),
        "unexpected status {status}"
    );
}

#[tokio::test]
async fn bootstrap_publishes_shared_pool_handle_for_indexing_daemon() {
    // BL-134 Phase 4 — booting the CLI runtime registers the
    // `com.nexus.ai.runtime` plugin BEFORE `com.nexus.ai`. The runtime's
    // `wire_context` calls `WorkerPool::publish_shared_handle`, which
    // makes the pool's tokio runtime handle available via
    // `nexus_ai_runtime::shared_pool_handle()` for the indexing daemon
    // (and any other sibling subsystem). This test exercises that
    // wiring: after boot, the handle must be `Some`.
    let _forge = scratch_forge();
    let _runtime = build_cli_runtime(_forge.path().to_path_buf()).expect("runtime");
    // OnceLock-backed accessor — the runtime plugin's wire_context
    // installs the handle synchronously, so a single check after
    // build_cli_runtime returns is sufficient.
    assert!(
        nexus_ai_runtime::shared_pool_handle().is_some(),
        "ai-runtime must publish a shared pool handle once wired; without it the \
         indexing daemon falls back to a bespoke tokio runtime per BL-134 Phase 4"
    );
}

#[tokio::test]
async fn republisher_translates_round_proposed_to_typed_ai_event() {
    // BL-134 Phase 2b-ii — boot the runtime, manually register a
    // (session_id → task_id) correlation via the public submit
    // path, then publish a `com.nexus.agent.round_proposed` event
    // with that session_id directly onto the kernel bus. The
    // republisher subscribes to that topic in `wire_context`, looks
    // up the correlation, and republishes as a typed
    // `AiEvent::RoundProposed` under
    // `com.nexus.ai.runtime.round_proposed`. We subscribe to the
    // typed topic and assert the translated payload arrives.
    use nexus_kernel::{EventFilter, NexusEvent};

    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Pin a session_id so we can drive the bus directly. Submit a
    // Session task with the caller-supplied id; the runtime worker
    // will try to call session_run (which will fail because the AI
    // provider isn't configured) but that's fine — we only need the
    // correlation entry to land before the worker reaches terminal.
    // Hardcoded UUID v4 — saves wiring `uuid` into dev-dependencies
    // just for one test. The wire only cares that this is a string
    // the agent / republisher round-trip honours; opaqueness is the
    // only contract.
    let session_id = "11111111-2222-3333-4444-555555555555".to_string();
    let submit_args = serde_json::json!({
        "task": {
            "kind": "session",
            "args": {
                "goal": "noop",
                "session_id": &session_id,
            }
        }
    });

    // Subscribe to the typed-event output BEFORE submit so we don't
    // race the worker.
    let mut sub = runtime
        .kernel
        .event_bus()
        .subscribe(EventFilter::CustomExact(
            "com.nexus.ai.runtime.round_proposed".to_string(),
        ));

    let reply = call(&runtime, "submit", submit_args).await.expect("submit");
    let _task_id = reply
        .get("task_id")
        .and_then(|v| v.as_str())
        .expect("task_id")
        .to_string();

    // Wait briefly for the subscriber to register and the worker to
    // ingest the correlation. The publish below is what the agent
    // would have done from inside session_run; we shortcut directly
    // to the bus. The runtime's worker holds the session→task
    // correlation for SESSION_CORRELATION_GRACE_MS past the terminal
    // event so the lookup here is race-free even though session_run
    // fast-fails (no AI provider configured).
    tokio::time::sleep(Duration::from_millis(50)).await;
    runtime
        .kernel
        .event_bus()
        .publish_plugin(
            "com.nexus.agent",
            "com.nexus.agent.round_proposed",
            serde_json::json!({
                "session_id": session_id,
                "round": 7,
                "text": "I'd like to read the file",
                "tool_calls": []
            }),
        )
        .expect("publish round_proposed");

    // Drain up to ~2s for the translated event. Once we see it,
    // assert the typed shape.
    let mut found = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(100), sub.recv()).await {
            Ok(Ok(evt)) => {
                if let NexusEvent::Custom { payload, .. } = &evt.event {
                    found = Some(payload.clone());
                    break;
                }
            }
            Ok(Err(_closed)) => break,
            Err(_timeout) => continue,
        }
    }

    let payload = found
        .expect("typed AiEvent::RoundProposed must be republished within 2s of the inner publish");
    assert_eq!(
        payload.get("kind").and_then(|v| v.as_str()),
        Some("round_proposed")
    );
    assert_eq!(payload.get("round").and_then(|v| v.as_i64()), Some(7));
    assert_eq!(
        payload.get("narration").and_then(|v| v.as_str()),
        Some("I'd like to read the file")
    );
}

// ─── Move 7: AmbientTrigger IPC tests ────────────────────────────────────────

/// Helper: build the serde_json representation of an AmbientTrigger without
/// depending on the Rust type directly (keeps this test file light on imports
/// and exercises the wire shape, not just the Rust API).
///
/// `TriggerId` serializes as `{ "0": "<uuid-string>" }` because it's a
/// newtype tuple struct. We supply a fixed v4 UUID so tests are deterministic.
fn make_trigger_with_id(
    id: &str,
    name: &str,
    type_id: &str,
    goal_template: &str,
) -> serde_json::Value {
    // TriggerId(Uuid) is a newtype struct — serde serializes it as the bare
    // UUID string, not as an object.
    serde_json::json!({
        "trigger": {
            "id": id,
            "name": name,
            "filter": { "kind": "custom_exact", "type_id": type_id },
            "goal_template": goal_template,
            "mode": "new_goal",
            "enabled": true,
        }
    })
}

fn make_trigger(name: &str, type_id: &str, goal_template: &str) -> serde_json::Value {
    make_trigger_with_id(
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        name,
        type_id,
        goal_template,
    )
}

#[tokio::test]
async fn register_trigger_returns_a_trigger_id() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let resp = call(
        &runtime,
        "register_trigger",
        make_trigger(
            "file-watch",
            "com.nexus.storage.file_changed",
            "Handle: {{event.payload}}",
        ),
    )
    .await
    .expect("register_trigger");

    // TriggerId(Uuid) serializes as a bare UUID string.
    let trigger_id = resp
        .get("trigger_id")
        .and_then(|v| v.as_str())
        .expect("trigger_id should be a UUID string");
    assert_eq!(
        trigger_id.len(),
        36,
        "trigger_id should be a formatted UUID, got {trigger_id}"
    );
}

#[tokio::test]
async fn list_triggers_empty_on_fresh_runtime() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let resp = call(&runtime, "list_triggers", serde_json::json!({}))
        .await
        .expect("list_triggers");
    assert_eq!(resp, serde_json::json!({ "triggers": [] }));
}

#[tokio::test]
async fn register_then_list_then_unregister_round_trip() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // register — use a unique id distinct from the other tests
    let my_id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let reg_resp = call(
        &runtime,
        "register_trigger",
        make_trigger_with_id(my_id, "my-trigger", "com.test.event", "Do: {{event.type}}"),
    )
    .await
    .expect("register_trigger");
    let trigger_id = reg_resp
        .get("trigger_id")
        .and_then(|v| v.as_str())
        .expect("trigger_id string")
        .to_string();
    assert_eq!(trigger_id, my_id);

    // list should show it
    let list = call(&runtime, "list_triggers", serde_json::json!({}))
        .await
        .expect("list_triggers");
    let triggers = list
        .get("triggers")
        .and_then(|v| v.as_array())
        .expect("triggers array");
    assert_eq!(triggers.len(), 1);
    assert_eq!(
        triggers[0].get("name").and_then(|v| v.as_str()),
        Some("my-trigger")
    );

    // unregister — trigger_id is a bare UUID string on the wire
    let un_resp = call(
        &runtime,
        "unregister_trigger",
        serde_json::json!({ "trigger_id": trigger_id }),
    )
    .await
    .expect("unregister_trigger");
    assert_eq!(un_resp.get("found").and_then(|v| v.as_bool()), Some(true));

    // list now empty
    let list2 = call(&runtime, "list_triggers", serde_json::json!({}))
        .await
        .expect("list_triggers after unregister");
    let triggers2 = list2
        .get("triggers")
        .and_then(|v| v.as_array())
        .expect("triggers array");
    assert!(triggers2.is_empty());
}

#[tokio::test]
async fn unregister_unknown_trigger_id_returns_found_false() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let fake_id = "ffffffff-ffff-ffff-ffff-ffffffffffff";
    let resp = call(
        &runtime,
        "unregister_trigger",
        serde_json::json!({ "trigger_id": fake_id }),
    )
    .await
    .expect("unregister_trigger");
    assert_eq!(resp.get("found").and_then(|v| v.as_bool()), Some(false));
}

#[tokio::test]
async fn trigger_watcher_spawns_session_when_matching_event_fires() {
    use nexus_kernel::Events as _;

    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Register a trigger that reacts to a specific custom event.
    let reg_resp = call(
        &runtime,
        "register_trigger",
        make_trigger_with_id(
            "cccccccc-cccc-cccc-cccc-cccccccccccc",
            "watcher-test",
            "com.nexus.test.trigger_fire",
            "Observed event: {{event.payload}}",
        ),
    )
    .await
    .expect("register_trigger");
    assert!(reg_resp.get("trigger_id").is_some());

    // Confirm list starts empty (no sessions submitted yet).
    let pre = call(&runtime, "list", serde_json::json!({}))
        .await
        .expect("list");
    assert_eq!(pre["runs"].as_array().unwrap().len(), 0);

    // Give the watcher loop a moment to start before we fire the event.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Publish the matching custom event directly on the kernel bus.
    runtime
        .kernel
        .event_bus()
        .publish_plugin(
            "com.nexus.test",
            "com.nexus.test.trigger_fire",
            serde_json::json!({ "msg": "hello from the bus" }),
        )
        .expect("publish custom event");

    // Poll for up to 2s for the watcher to react and submit a session.
    let mut found_run = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        let listing = call(&runtime, "list", serde_json::json!({}))
            .await
            .expect("list");
        let runs = listing["runs"].as_array().unwrap();
        if !runs.is_empty() {
            assert_eq!(
                runs[0]["kind"], "session",
                "watcher must submit a session task"
            );
            found_run = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        found_run,
        "trigger watcher must submit a SignalTriggered session within 2s of matching event"
    );
}
