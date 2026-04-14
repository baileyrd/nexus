//! Runtime bootstrap smoke tests.
//!
//! These prove the bootstrap wires kernel + loader + core plugins + invoker
//! together without crashing. They do NOT yet exercise any IPC command —
//! those handlers land in later phases.

use std::time::Duration;

use nexus_bootstrap::{build_cli_runtime, build_tui_runtime, CLI_PLUGIN_ID, TUI_PLUGIN_ID};
use nexus_kernel::{IpcError, PluginContext};

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    // Kernel::new will create .forge itself; just ensure the parent exists.
    std::fs::create_dir_all(dir.path()).unwrap();
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
async fn cli_runtime_ipc_call_routes_to_registered_core_plugins() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build cli runtime");

    // None of the subsystem plugins register IPC commands yet, so every call
    // should come back as CommandNotFound — which proves routing works:
    // the dispatcher looked up the target plugin and asked for the command.
    let err = runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "query_files",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == "com.nexus.storage" && command == "query_files"
        ),
        "expected CommandNotFound for storage/query_files, got {err:?}"
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
