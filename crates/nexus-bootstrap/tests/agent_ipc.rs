//! End-to-end tests for the agent core plugin (`com.nexus.agent`)
//! driven through the kernel IPC surface.
//!
//! Pre-ADR-0025 this file pinned `run_plan`, `execute_step`, and the
//! four `com.nexus.agent.*` kernel-bus events that drove live plan
//! progress for the legacy planner. Phase 2 retired all of those —
//! callers should drive `session_run` (covered separately under
//! `nexus-agent`'s in-tree session tests). The only contract still
//! pinned here is the path-traversal guard on `history_get`, which
//! continues to surface pre-Phase-2a plan-history JSON.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{IpcError, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const AGENT_PLUGIN_ID: &str = "com.nexus.agent";

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
        .ipc_call(AGENT_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
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

/// ADR 0025 Phase 2 — retired handlers must surface CommandNotFound
/// rather than silently routing somewhere unexpected. Pin the most-called
/// retirees so a future "let's restore one for back-compat" attempt has
/// to update this test.
///
/// Note: `delegate` was originally retired by ADR 0025, then re-introduced
/// for DG-37 (2026-05-12) on top of the new session model — it now routes
/// through `handle_session_run` rather than the old BL-027 orchestrator.
/// `parallel` / `pipeline` / `trace_get` stay retired (caller composition
/// patterns over `delegate` cover the same use cases).
#[tokio::test]
async fn retired_handlers_return_command_not_found() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    for cmd in ["run", "run_plan", "execute_step", "parallel", "pipeline", "trace_get"] {
        let err = call(&runtime, cmd, serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(
            matches!(err, IpcError::CommandNotFound { .. }),
            "{cmd}: expected CommandNotFound, got {err:?}"
        );
    }
}
