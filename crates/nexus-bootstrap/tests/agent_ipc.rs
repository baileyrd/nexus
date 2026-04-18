//! End-to-end tests for the agent core plugin (`com.nexus.agent`)
//! driven through the kernel IPC surface.
//!
//! Pins the contract the ChatPanel + AgentHistoryPanel depend on:
//! `run_plan`, `execute_step`, history list/get/delete, and the four
//! `com.nexus.agent.*` kernel-bus events that drive live plan
//! progress in the UI. `plan` and `run` are deliberately skipped —
//! they require a configured LLM provider, which isn't available in
//! tests.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{EventFilter, IpcError, NexusEvent, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const AGENT_PLUGIN_ID: &str = "com.nexus.agent";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

/// A two-step plan: one tool-call step that writes a file through
/// storage IPC, one informational step with no tool_call.
fn preset_plan() -> serde_json::Value {
    // plan_id is used as the history filename — stick to alphanumerics
    // + `-` / `_` so it passes the persistence safety check.
    serde_json::json!({
        "id": "plan-smoke-001",
        "goal": "smoke test",
        "steps": [
            {
                "id": "step-1",
                "description": "write a scratch file",
                "tool_call": {
                    "target_plugin_id": "com.nexus.storage",
                    "command_id": "write_file",
                    "args": {
                        "path": "agent-smoke.txt",
                        "bytes": [65, 66, 67],
                    }
                }
            },
            {
                "id": "step-2",
                "description": "informational — no tool call",
                "tool_call": null
            }
        ]
    })
}

async fn call(
    runtime: &nexus_bootstrap::Runtime,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    runtime
        .context
        .ipc_call(AGENT_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn run_plan_executes_steps_and_persists_history() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let obs = call(
        &runtime,
        "run_plan",
        serde_json::json!({ "plan": preset_plan() }),
    )
    .await
    .expect("run_plan ok");

    assert_eq!(obs["plan_id"], "plan-smoke-001");
    assert_eq!(obs["success"], true);
    let steps = obs["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["status"], "ok");
    assert_eq!(steps[1]["status"], "ok");

    // History list picks up the run.
    let list = call(&runtime, "history_list", serde_json::json!({}))
        .await
        .unwrap();
    let entries = list.as_array().expect("history_list returns array");
    let entry = entries
        .iter()
        .find(|e| e["plan_id"] == "plan-smoke-001")
        .expect("history entry present");
    assert_eq!(entry["steps"], 2);
    assert_eq!(entry["success"], true);

    // history_get round-trips the full record.
    let rec = call(
        &runtime,
        "history_get",
        serde_json::json!({ "plan_id": "plan-smoke-001" }),
    )
    .await
    .unwrap();
    assert_eq!(rec["plan_id"], "plan-smoke-001");
    assert_eq!(rec["observation"]["success"], true);

    // history_delete removes it.
    let del = call(
        &runtime,
        "history_delete",
        serde_json::json!({ "plan_id": "plan-smoke-001" }),
    )
    .await
    .unwrap();
    assert_eq!(del["deleted"], true);
    let list_after = call(&runtime, "history_list", serde_json::json!({}))
        .await
        .unwrap();
    assert!(
        !list_after
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["plan_id"] == "plan-smoke-001"),
        "history entry should be gone after delete"
    );
}

#[tokio::test]
async fn execute_step_runs_one_step_at_a_time() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let result = call(
        &runtime,
        "execute_step",
        serde_json::json!({ "plan": preset_plan(), "index": 0 }),
    )
    .await
    .expect("execute_step ok");

    assert_eq!(result["step_id"], "step-1");
    assert_eq!(result["status"], "ok");

    // Second step is informational — no tool call — still completes ok.
    let result = call(
        &runtime,
        "execute_step",
        serde_json::json!({ "plan": preset_plan(), "index": 1 }),
    )
    .await
    .expect("execute_step idx 1 ok");
    assert_eq!(result["step_id"], "step-2");
    assert_eq!(result["status"], "ok");
}

#[tokio::test]
async fn run_plan_emits_bus_events_in_order() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Subscribe BEFORE calling run_plan so the broadcast channel
    // buffers every emitted event. run_plan awaits to completion, then
    // we drain what the bus buffered.
    let mut sub = runtime
        .context
        .subscribe(EventFilter::CustomPrefix("com.nexus.agent.".into()));

    let obs = call(
        &runtime,
        "run_plan",
        serde_json::json!({ "plan": preset_plan() }),
    )
    .await
    .expect("run_plan ok");
    assert_eq!(obs["success"], true);

    let mut seen: Vec<String> = Vec::new();
    while let Some(ev) = sub.try_recv().expect("try_recv ok") {
        if let NexusEvent::Custom { type_id, .. } = &ev.event {
            seen.push(type_id.clone());
        }
    }

    // Order: run_start → step_start → step_done (×2) → run_done.
    assert_eq!(seen.first().map(String::as_str), Some("com.nexus.agent.run_start"));
    assert_eq!(
        seen.last().map(String::as_str),
        Some("com.nexus.agent.run_done")
    );
    let step_starts = seen
        .iter()
        .filter(|t| t.as_str() == "com.nexus.agent.step_start")
        .count();
    let step_dones = seen
        .iter()
        .filter(|t| t.as_str() == "com.nexus.agent.step_done")
        .count();
    assert_eq!(step_starts, 2, "one step_start per step; got {seen:?}");
    assert_eq!(step_dones, 2, "one step_done per step; got {seen:?}");
}

#[tokio::test]
async fn history_get_errors_for_invalid_plan_id() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(
        &runtime,
        "history_get",
        serde_json::json!({ "plan_id": "../escape" }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}
