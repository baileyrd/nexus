//! End-to-end tests for the theme plugin's IPC surface.
//!
//! Proves that `com.nexus.theme`'s handlers round-trip correctly through
//! the kernel, and that mutations publish `com.nexus.theme.changed`
//! events on the bus so plugins (and the shell frontend) can react.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{EventFilter, IpcError, NexusEvent, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const THEME_PLUGIN_ID: &str = "com.nexus.theme";

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
        .ipc_call(THEME_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn get_available_themes_returns_builtins_through_ipc() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let value = call(&runtime, "get_available_themes", serde_json::json!({}))
        .await
        .expect("get_available_themes dispatches");

    let list = value.as_array().expect("array of themes");
    assert!(list.iter().any(|t| t["id"] == "nexus-light"));
    assert!(list.iter().any(|t| t["id"] == "nexus-dark"));
}

#[tokio::test]
async fn apply_theme_publishes_changed_event_on_bus() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    // Subscribe before dispatching so the event is captured.
    let mut sub = runtime
        .kernel
        .event_bus()
        .subscribe(EventFilter::CustomPrefix("com.nexus.theme.".to_string()));

    call(
        &runtime,
        "apply_theme",
        serde_json::json!({ "id": "nexus-dark" }),
    )
    .await
    .expect("apply_theme dispatches");

    // Give the bus a tick to deliver — subscribe is a broadcast channel.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let event = sub
        .try_recv()
        .expect("bus recv")
        .expect("event should have been published");

    match &event.event {
        NexusEvent::Custom {
            type_id, payload, ..
        } => {
            assert_eq!(type_id, "com.nexus.theme.changed");
            assert_eq!(payload["theme_id"], "nexus-dark");
        }
        other => panic!("expected Custom, got {other:?}"),
    }
}

#[tokio::test]
async fn get_theme_config_reflects_apply_theme_through_ipc() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    call(
        &runtime,
        "apply_theme",
        serde_json::json!({ "id": "nexus-dark" }),
    )
    .await
    .expect("apply_theme dispatches");

    let cfg = call(&runtime, "get_theme_config", serde_json::json!({}))
        .await
        .expect("get_theme_config dispatches");

    assert_eq!(cfg["theme_id"], "nexus-dark");
}

#[tokio::test]
async fn unknown_theme_command_returns_command_not_found() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let err = call(&runtime, "not-a-real-command", serde_json::json!({}))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == THEME_PLUGIN_ID && command == "not-a-real-command"
        ),
        "got {err:?}"
    );
}
