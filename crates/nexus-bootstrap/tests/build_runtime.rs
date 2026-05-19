//! Runtime bootstrap smoke tests.
//!
//! These prove the bootstrap wires kernel + loader + core plugins + invoker
//! together without crashing. They do NOT yet exercise any IPC command —
//! those handlers land in later phases.

use std::time::Duration;

use nexus_bootstrap::{build_cli_runtime, build_tui_runtime, CLI_PLUGIN_ID, TUI_PLUGIN_ID};
use nexus_kernel::{Identity as _, Ipc as _, IpcError};

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    // Storage plugin's on_init opens a StorageEngine, which requires a fully
    // initialized forge (schema, WAL, etc.), not just an empty dir.
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

#[test]
fn cli_runtime_builds_and_reports_expected_identity() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build cli runtime");
    assert_eq!(runtime.context.plugin_id(), CLI_PLUGIN_ID);
}

#[test]
fn tui_runtime_builds_and_reports_expected_identity() {
    let forge = scratch_forge();
    let runtime = build_tui_runtime(forge.path().to_path_buf()).expect("build tui runtime");
    assert_eq!(runtime.context.plugin_id(), TUI_PLUGIN_ID);
}

#[tokio::test]
async fn cli_runtime_storage_query_files_roundtrips() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("build cli runtime");

    let value = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "query_files",
            serde_json::json!({}),
            Duration::from_secs(5),
        )
        .await
        .expect("query_files dispatches cleanly");

    assert!(value.is_array(), "expected array, got {value}");
    assert_eq!(
        value.as_array().unwrap().len(),
        0,
        "fresh forge should have no files"
    );
}

#[tokio::test]
async fn cli_runtime_storage_unknown_command_returns_command_not_found() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("build cli runtime");

    let err = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "not-a-real-command",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == "com.nexus.storage" && command == "not-a-real-command"
        ),
        "got {err:?}"
    );
}

#[tokio::test]
async fn cli_runtime_reports_unknown_plugin() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build cli runtime");

    let err = runtime
        .context
        .ipc_call(
            "com.nexus.nonexistent",
            "whatever",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, IpcError::PluginNotFound { .. }),
        "got {err:?}"
    );
}
