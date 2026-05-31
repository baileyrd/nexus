//! End-to-end tests for the workflow core plugin
//! (`com.nexus.workflow`) driven through the kernel IPC surface.
//!
//! Pins the contract the WorkflowsPanel + CLI `nexus workflow …`
//! command depend on: `list`, `get`, `reload`, `validate`, `run`.

use std::fs;
use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{Ipc as _, IpcError};
use nexus_workflow::PLUGIN_ID;

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

// Manual trigger so the plugin's cron scheduler doesn't spawn a
// background task during tests.
const WF_NOOP: &str = r#"
[workflow]
name = "Greet"
description = "noop smoke"

[trigger]
type = "manual"

[[steps]]
name = "hello"
type = "noop"
"#;

const WF_BAD: &str = "not valid toml {{";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

fn write_workflow(root: &std::path::Path, relpath: &str, body: &str) {
    let abs = root.join(".workflows").join(relpath);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(abs, body).unwrap();
}

async fn call(
    runtime: &nexus_bootstrap::Runtime,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    runtime
        .context
        .ipc_call(PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn list_returns_workflows_on_disk() {
    let forge = scratch_forge();
    write_workflow(forge.path(), "greet.workflow.toml", WF_NOOP);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "list", serde_json::json!({})).await.unwrap();
    let arr = v.as_array().expect("list returns array");
    assert!(
        arr.iter().any(|w| w["workflow"]["name"] == "Greet"),
        "expected `Greet`; got {arr:?}"
    );
}

#[tokio::test]
async fn get_returns_workflow_and_errors_for_missing() {
    let forge = scratch_forge();
    write_workflow(forge.path(), "greet.workflow.toml", WF_NOOP);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "get", serde_json::json!({ "name": "Greet" }))
        .await
        .unwrap();
    assert_eq!(v["workflow"]["name"], "Greet");
    assert_eq!(v["trigger"]["type"], "manual");

    let err = call(&runtime, "get", serde_json::json!({ "name": "Nope" }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn reload_picks_up_new_files() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let before: Vec<serde_json::Value> =
        serde_json::from_value(call(&runtime, "list", serde_json::json!({})).await.unwrap())
            .unwrap();
    let before_count = before.len();

    write_workflow(forge.path(), "greet.workflow.toml", WF_NOOP);
    let v = call(&runtime, "reload", serde_json::json!({}))
        .await
        .unwrap();
    let loaded = v["loaded"].as_u64().expect("loaded is a u64");
    assert_eq!(loaded, (before_count + 1) as u64);

    let after: Vec<serde_json::Value> =
        serde_json::from_value(call(&runtime, "list", serde_json::json!({})).await.unwrap())
            .unwrap();
    assert!(after.iter().any(|w| w["workflow"]["name"] == "Greet"));
}

#[tokio::test]
async fn validate_accepts_good_toml_and_rejects_bad() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "validate", serde_json::json!({ "text": WF_NOOP }))
        .await
        .unwrap();
    assert_eq!(v["workflow"]["name"], "Greet");

    let err = call(&runtime, "validate", serde_json::json!({ "text": WF_BAD }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn run_executes_steps_and_returns_outcome() {
    let forge = scratch_forge();
    write_workflow(forge.path(), "greet.workflow.toml", WF_NOOP);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "run", serde_json::json!({ "name": "Greet" }))
        .await
        .expect("run ok");

    assert_eq!(v["workflow_name"], "Greet");
    assert_eq!(v["success"], true);
    let steps = v["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["step_id"], "hello");
    assert_eq!(steps[0]["status"], "ok");
}

#[tokio::test]
async fn run_interpolates_variables_into_ipc_step_args() {
    // End-to-end proof that `variables.trigger.*` reaches the step
    // dispatcher through JSON flattening + TOML substitution.
    const WF_READ: &str = r#"
[workflow]
name = "ReadIt"

[trigger]
type = "manual"

[[steps]]
name = "read"
type = "ipc"
target = "com.nexus.storage"
command = "read_file"
[steps.args]
path = "${trigger.path}"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "readit.workflow.toml", WF_READ);
    // Seed a real file at notes/hello.md that the interpolated step
    // should read. Using com.nexus.storage::write_file keeps the write
    // on the same code path the reader will hit.
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "write_file",
            serde_json::json!({
                "path": "notes/hello.md",
                "bytes": b"HELLO".to_vec(),
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect("seed write");

    let v = call(
        &runtime,
        "run",
        serde_json::json!({
            "name": "ReadIt",
            "variables": { "trigger": { "path": "notes/hello.md" } }
        }),
    )
    .await
    .expect("run ok");

    assert_eq!(v["success"], true, "got {v:?}");
    let bytes = v["steps"][0]["response"]["bytes"]
        .as_array()
        .expect("read_file returns {bytes: [...]}");
    let content: Vec<u8> = bytes
        .iter()
        .map(|n| u8::try_from(n.as_u64().unwrap()).unwrap())
        .collect();
    assert_eq!(std::str::from_utf8(&content).unwrap(), "HELLO");
}

#[tokio::test]
async fn run_short_circuits_when_condition_false() {
    // Condition evaluates false (regex mismatch) → executor never
    // dispatches the step, the run reports `condition_skipped: true`
    // and `success: true`.
    const WF_GATED: &str = r#"
[workflow]
name = "Gated"

[trigger]
type = "manual"

[condition]
type = "regex_match"
source = "${trigger.path}"
pattern = "^notes/.*\\.md$"

[[steps]]
name = "read"
type = "ipc"
target = "com.nexus.storage"
command = "read_file"
[steps.args]
path = "ghost.md"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "gated.workflow.toml", WF_GATED);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // trigger.path = other/x.txt → pattern fails → gate closed.
    let v = call(
        &runtime,
        "run",
        serde_json::json!({
            "name": "Gated",
            "variables": { "trigger": { "path": "other/x.txt" } }
        }),
    )
    .await
    .expect("run ok");

    assert_eq!(v["success"], true);
    assert_eq!(v["condition_skipped"], true);
    assert_eq!(
        v["steps"].as_array().map(Vec::len).unwrap_or(usize::MAX),
        0,
        "condition-skipped run must emit zero step outcomes"
    );
}

#[tokio::test]
async fn run_dispatches_when_condition_true() {
    // Same workflow as the gated test, trigger.path matches the
    // pattern → executor runs the step as usual (fails read_file on
    // a non-existent file, but that's an ordinary step failure, not
    // a gate close).
    const WF_GATED: &str = r#"
[workflow]
name = "OpenGate"

[trigger]
type = "manual"

[condition]
type = "regex_match"
source = "${trigger.path}"
pattern = "^notes/.*\\.md$"

[[steps]]
name = "read"
type = "ipc"
target = "com.nexus.storage"
command = "read_file"
[steps.args]
path = "${trigger.path}"
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "opengate.workflow.toml", WF_GATED);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "write_file",
            serde_json::json!({
                "path": "notes/cond.md",
                "bytes": b"OK".to_vec(),
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect("seed write");

    let v = call(
        &runtime,
        "run",
        serde_json::json!({
            "name": "OpenGate",
            "variables": { "trigger": { "path": "notes/cond.md" } }
        }),
    )
    .await
    .expect("run ok");

    assert_eq!(v["success"], true);
    assert!(
        !v["condition_skipped"].as_bool().unwrap_or(false),
        "open gate must not flag condition_skipped"
    );
    assert_eq!(v["steps"][0]["status"], "ok");
}

#[tokio::test]
async fn file_event_trigger_fires_workflow_when_watched_path_changes() {
    // End-to-end: the storage plugin's OS watcher sees a new file at
    // notes/observed.md, emits com.nexus.storage.file_created on the
    // kernel bus, the workflow plugin's file_event listener picks it
    // up, matches watch_dir/pattern, and dispatches
    // com.nexus.workflow::run. The step writes a marker file whose
    // appearance proves the whole loop closed.
    //
    // The marker path lives outside watch_dir and doesn't match the
    // `.md$` pattern, so the workflow doesn't retrigger itself.
    const WF: &str = r#"
[workflow]
name = "OnFileCreate"

[trigger]
type = "file_event"
watch_dir = "notes/"
pattern = "\\.md$"
events = ["created", "modified"]

[[steps]]
name = "mark"
type = "ipc"
target = "com.nexus.storage"
command = "write_file"
[steps.args]
path = "fired.marker"
bytes = [70, 73, 82, 69, 68]  # "FIRED" as bytes
"#;
    let forge = scratch_forge();
    write_workflow(forge.path(), "onfile.workflow.toml", WF);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Give the storage watcher and the workflow's file_event task a
    // tick to arm before we drop the triggering file.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let notes_dir = forge.path().join("notes");
    std::fs::create_dir_all(&notes_dir).unwrap();
    std::fs::write(notes_dir.join("observed.md"), b"hello").unwrap();

    // Poll for the marker. Storage watcher → bus → workflow trigger →
    // ipc_call → write_file can take a couple of debounce cycles on
    // slow CI.
    let marker = forge.path().join("fired.marker");
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        marker.exists(),
        "file_event trigger never fired — marker `{}` was not written",
        marker.display()
    );
    let contents = std::fs::read(&marker).unwrap();
    assert_eq!(&contents, b"FIRED");
    let _ = runtime; // keep runtime alive until assertion passes
}

#[tokio::test]
async fn run_rejects_non_object_variables() {
    let forge = scratch_forge();
    write_workflow(forge.path(), "greet.workflow.toml", WF_NOOP);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(
        &runtime,
        "run",
        serde_json::json!({ "name": "Greet", "variables": "nope" }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn run_errors_for_unknown_workflow_name() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(&runtime, "run", serde_json::json!({ "name": "ghost" }))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}
