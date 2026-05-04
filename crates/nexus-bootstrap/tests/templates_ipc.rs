//! End-to-end tests for the templates core plugin (`com.nexus.templates`)
//! driven through the kernel IPC surface. Pins the surface the shell
//! command palette and CLI consume.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::PluginContext;
use nexus_templates::PLUGIN_ID;

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

async fn call(
    runtime: &nexus_bootstrap::Runtime,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, nexus_kernel::IpcError> {
    runtime
        .context
        .ipc_call(PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn list_returns_built_in_templates() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");

    let result = call(&runtime, "list", serde_json::json!({}))
        .await
        .expect("list ok");
    let arr = result.as_array().expect("array");
    assert!(
        arr.len() >= 4,
        "expected built-in templates, got {} entries",
        arr.len()
    );
    let names: Vec<&str> = arr
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(names.contains(&"daily-journal"), "names: {names:?}");
    assert!(names.contains(&"meeting-notes"), "names: {names:?}");
    assert!(names.contains(&"notion-page"), "names: {names:?}");
}

#[tokio::test]
async fn apply_writes_a_file_in_the_forge() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");

    let result = call(
        &runtime,
        "apply",
        serde_json::json!({
            "name": "notion-page",
            "args": { "title": "IPC Test", "status": "draft", "tags": "" }
        }),
    )
    .await
    .expect("apply ok");

    let abs = result["absolute_path"].as_str().expect("absolute_path");
    let body = std::fs::read_to_string(abs).expect("written file readable");
    assert!(body.contains("# IPC Test"), "body:\n{body}");
}

#[tokio::test]
async fn render_dry_runs_without_writing() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");

    let result = call(
        &runtime,
        "render",
        serde_json::json!({ "name": "daily-journal" }),
    )
    .await
    .expect("render ok");

    assert!(result["body"].as_str().is_some());
    let target = result["target_path"].as_str().expect("target_path");
    assert!(target.starts_with("daily/"), "target was {target}");

    // Forge daily/ should not exist — render is a dry-run.
    assert!(!dir.path().join("daily").exists());
}

#[tokio::test]
async fn unknown_template_errors() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");

    let err = call(&runtime, "get", serde_json::json!({ "name": "no-such" }))
        .await
        .expect_err("expected error for missing template");
    let msg = format!("{err}");
    assert!(msg.contains("no-such"), "{msg}");
}

#[tokio::test]
async fn user_template_in_forge_overrides_builtin() {
    let dir = scratch_forge();
    let templates_dir = dir.path().join(".forge/templates");
    std::fs::create_dir_all(&templates_dir).unwrap();
    std::fs::write(
        templates_dir.join("daily-journal.template.md"),
        "---\nname: daily-journal\ndescription: My override\n---\nMine.\n",
    )
    .unwrap();

    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");
    let result = call(
        &runtime,
        "get",
        serde_json::json!({ "name": "daily-journal" }),
    )
    .await
    .expect("get ok");
    assert_eq!(
        result.get("description").and_then(|v| v.as_str()),
        Some("My override")
    );
}
