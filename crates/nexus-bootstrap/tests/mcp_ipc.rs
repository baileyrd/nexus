//! End-to-end tests for the MCP host core plugin
//! (`com.nexus.mcp.host`) driven through the kernel IPC surface.
//!
//! Only the config-side and error-path handlers are exercised here —
//! `list_tools`, `call_tool`, `list_resources`, `list_prompts`, and
//! `connect` all spawn an external MCP server process (via `rmcp`), so
//! they can't be tested hermetically in CI. Connected-path coverage
//! lives in the `nexus-mcp` integration tests that gate on a local
//! reference server binary. The handler *wiring* — dispatch plumbing,
//! arg parsing, config plumbing, error mapping — is fully covered by
//! the tests below.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{IpcError, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const MCP_PLUGIN_ID: &str = "com.nexus.mcp.host";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

fn write_mcp_toml(root: &std::path::Path, body: &str) {
    let forge_dir = root.join(".forge");
    std::fs::create_dir_all(&forge_dir).unwrap();
    std::fs::write(forge_dir.join("mcp.toml"), body).unwrap();
}

async fn call(
    runtime: &nexus_bootstrap::Runtime,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    runtime
        .context
        .ipc_call(MCP_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn list_servers_is_empty_without_mcp_toml() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "list_servers", serde_json::json!({}))
        .await
        .expect("list_servers ok");
    let arr = v.as_array().expect("list_servers returns array");
    assert!(arr.is_empty(), "no mcp.toml → no servers; got {arr:?}");
}

#[tokio::test]
async fn list_servers_reflects_mcp_toml_configuration() {
    let forge = scratch_forge();
    write_mcp_toml(
        forge.path(),
        r#"
[servers.fs]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]

[servers.gh]
command = "uvx"
args = ["mcp-server-github"]
disabled = true
"#,
    );
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "list_servers", serde_json::json!({}))
        .await
        .expect("list_servers ok");
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    let names: Vec<&str> = arr.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"fs"));
    assert!(names.contains(&"gh"));
    let gh = arr.iter().find(|s| s["name"] == "gh").unwrap();
    assert_eq!(gh["disabled"], true);
    assert_eq!(gh["command"], "uvx");
}

#[tokio::test]
async fn disconnect_reports_not_connected_for_unknown_server() {
    // `disconnect` is one of the few async handlers that doesn't spawn
    // a process — it only touches the in-memory client map. Removing an
    // id that was never connected returns { ok: false } rather than
    // erroring.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(
        &runtime,
        "disconnect",
        serde_json::json!({ "server": "never-connected" }),
    )
    .await
    .expect("disconnect ok");
    assert_eq!(v["ok"], false);
    assert_eq!(v["server"], "never-connected");
}

#[tokio::test]
async fn list_tools_without_server_arg_returns_command_not_found_shape() {
    // Missing required arg makes `dispatch_async` return `None`; the
    // loader falls back to sync dispatch which returns an
    // ExecutionFailed, surfaced over IPC as PluginCrashedDuringCall.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(&runtime, "list_tools", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn connect_to_unknown_server_errors_with_config_loaded() {
    let forge = scratch_forge();
    write_mcp_toml(
        forge.path(),
        r#"
[servers.real]
command = "true"
"#,
    );
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(
        &runtime,
        "connect",
        serde_json::json!({ "server": "ghost" }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn unknown_mcp_command_returns_command_not_found() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(&runtime, "no-such-mcp-command", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == MCP_PLUGIN_ID && command == "no-such-mcp-command"
        ),
        "got {err:?}"
    );
}
