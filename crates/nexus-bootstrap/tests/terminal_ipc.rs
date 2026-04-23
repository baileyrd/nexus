//! End-to-end tests for the terminal core plugin (`com.nexus.terminal`)
//! driven through the kernel IPC surface.
//!
//! Pins the contract the Terminal panel + saved-commands sidebar depend
//! on: `create_session`, `list_sessions`, `send_input`, `pump`,
//! `read_output`, `close_session`, and the saved-command CRUD surface.
//!
//! Shell-spawning tests are gated to Unix because the CI runners that
//! execute `scripts/test_all.sh` run under WSL, and the spawnable
//! fallback shell (`/bin/sh`) is only guaranteed there. Non-Unix
//! coverage lives in the platform-agnostic subset (empty list, unknown
//! command, saved CRUD).

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{IpcError, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const TERMINAL_PLUGIN_ID: &str = "com.nexus.terminal";

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
        .ipc_call(TERMINAL_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn list_sessions_is_empty_on_fresh_runtime() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "list_sessions", serde_json::json!({}))
        .await
        .expect("list_sessions ok");
    let arr = v.as_array().expect("list_sessions returns array");
    assert!(arr.is_empty(), "fresh runtime has no sessions; got {arr:?}");
}

#[tokio::test]
async fn unknown_terminal_command_returns_command_not_found() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(&runtime, "no-such-terminal-command", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == TERMINAL_PLUGIN_ID && command == "no-such-terminal-command"
        ),
        "got {err:?}"
    );
}

#[tokio::test]
async fn get_session_info_for_unknown_id_errors() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(
        &runtime,
        "get_session_info",
        serde_json::json!({ "id": "no-such-session" }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn saved_commands_crud_roundtrips_through_ipc() {
    // The bootstrap wires the terminal plugin with an SQLite-backed
    // saved-commands store at `<forge>/.forge/procmgr.sqlite`. Verify
    // create → list → delete roundtrips through IPC.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Empty to start.
    let before = call(&runtime, "saved_list", serde_json::json!({}))
        .await
        .expect("saved_list ok");
    assert_eq!(
        before.as_array().map(Vec::len),
        Some(0),
        "fresh forge should have no saved commands"
    );

    // Create one. Field names must match `nexus_terminal::saved::SavedCommand`.
    let created = call(
        &runtime,
        "saved_create",
        serde_json::json!({
            "slug": "list-here",
            "name": "List here",
            "shell": "/bin/sh",
            "shell_cmd": "ls -la",
            "working_dir": null,
            "env_vars": {},
            "env_file": null,
            "icon": "terminal",
            "auto_restart": false,
            "auto_restart_delay_ms": 0,
            "memory_limit_mb": null,
            "sidebar_order": null,
            "pre_commands": [],
            "created_at": 0,
            "updated_at": 0,
        }),
    )
    .await
    .expect("saved_create ok");
    assert_eq!(created["slug"], "list-here");

    let after = call(&runtime, "saved_list", serde_json::json!({}))
        .await
        .expect("saved_list after create");
    let arr = after.as_array().unwrap();
    assert!(
        arr.iter().any(|r| r["slug"] == "list-here"),
        "saved list missing created row; got {arr:?}"
    );

    // Delete it.
    let deleted = call(
        &runtime,
        "saved_delete",
        serde_json::json!({ "slug": "list-here" }),
    )
    .await
    .expect("saved_delete ok");
    assert_eq!(deleted["slug"], "list-here");

    let empty = call(&runtime, "saved_list", serde_json::json!({}))
        .await
        .expect("saved_list after delete");
    assert_eq!(empty.as_array().map(Vec::len), Some(0));
}

#[cfg(unix)]
#[tokio::test]
async fn create_session_then_list_and_close_roundtrip() {
    // Spawns `/bin/sh` through the PTY server; only safe on Unix.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let created = call(
        &runtime,
        "create_session",
        serde_json::json!({
            "name": "smoke",
            "shell": "/bin/sh",
            "shell_args": [],
        }),
    )
    .await
    .expect("create_session ok");
    let id = created["id"]
        .as_str()
        .expect("create_session returns { id }");

    let list = call(&runtime, "list_sessions", serde_json::json!({}))
        .await
        .expect("list_sessions ok");
    let arr = list.as_array().expect("array");
    assert!(
        arr.iter().any(|s| s["id"] == id),
        "list should include freshly created session; got {arr:?}"
    );

    // Close it — should return null on success, not error.
    call(
        &runtime,
        "close_session",
        serde_json::json!({ "id": id }),
    )
    .await
    .expect("close_session ok");
}

#[cfg(unix)]
#[tokio::test]
async fn send_input_then_pump_sees_output_bytes() {
    // Spawn sh, echo a marker, pump, and assert we drained bytes.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let created = call(
        &runtime,
        "create_session",
        serde_json::json!({
            "shell": "/bin/sh",
            "shell_args": [],
        }),
    )
    .await
    .expect("create_session");
    let id = created["id"].as_str().unwrap().to_string();

    call(
        &runtime,
        "send_input",
        serde_json::json!({ "id": id, "input": "echo hello-from-ipc" }),
    )
    .await
    .expect("send_input ok");

    // Pump with a generous window so slow CI still drains.
    let mut total = 0u64;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline && total == 0 {
        let v = call(
            &runtime,
            "pump",
            serde_json::json!({ "id": id, "timeout_ms": 300 }),
        )
        .await
        .expect("pump ok");
        total += v["bytes"].as_u64().unwrap_or(0);
    }
    assert!(total > 0, "pump should have drained echo output");

    // Clean up.
    let _ = call(&runtime, "close_session", serde_json::json!({ "id": id })).await;
}
