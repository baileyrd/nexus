//! End-to-end tests for the skills core plugin (`com.nexus.skills`)
//! driven through the kernel IPC surface.
//!
//! Pins the contract the SkillsPanel + agent planner consume:
//! `list`, `get`, `render`, `reload`. Mirrors the IPC path production
//! callers use — `runtime.context.ipc_call(PLUGIN_ID, command, args, _)`.

use std::fs;
use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{Ipc as _, IpcError};
use nexus_skills::PLUGIN_ID;

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const SKILL_TONE: &str = r#"---
name: Tone
id: tone
description: applies a tone
version: 1.0.0
author: test
created: 2026-04-18
applicable_contexts: [ai-chat]
triggers: ["tone check"]
parameters:
  - name: tone
    type: string
    default: friendly
---
Write in a {{ tone }} style.
"#;

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

fn write_skill(root: &std::path::Path, relpath: &str, body: &str) {
    let abs = root.join(".forge").join("skills").join(relpath);
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
async fn list_includes_skills_on_disk() {
    let forge = scratch_forge();
    write_skill(forge.path(), "tone.skill.md", SKILL_TONE);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "list", serde_json::json!({})).await.unwrap();
    let arr = v.as_array().expect("list returns array");
    assert!(
        arr.iter().any(|s| s["id"] == "tone"),
        "expected `tone` in list; got {arr:?}"
    );
}

#[tokio::test]
async fn get_returns_skill_and_errors_for_missing() {
    let forge = scratch_forge();
    write_skill(forge.path(), "tone.skill.md", SKILL_TONE);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "get", serde_json::json!({ "id": "tone" }))
        .await
        .unwrap();
    assert_eq!(v["id"], "tone");
    assert_eq!(v["name"], "Tone");

    let err = call(&runtime, "get", serde_json::json!({ "id": "missing" }))
        .await
        .unwrap_err();
    // Handler returns `PluginError::ExecutionFailed`; the loader
    // collapses any non-NotFound plugin error to PluginCrashedDuringCall
    // at the IPC boundary.
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn render_substitutes_values_and_defaults() {
    let forge = scratch_forge();
    write_skill(forge.path(), "tone.skill.md", SKILL_TONE);
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // With an explicit override
    let v = call(
        &runtime,
        "render",
        serde_json::json!({ "id": "tone", "values": { "tone": "formal" } }),
    )
    .await
    .unwrap();
    assert_eq!(v["id"], "tone");
    assert!(
        v["body"].as_str().unwrap().contains("formal"),
        "body should reflect override; got {:?}",
        v["body"]
    );

    // Without values — falls back to declared default
    let v = call(&runtime, "render", serde_json::json!({ "id": "tone" }))
        .await
        .unwrap();
    assert!(
        v["body"].as_str().unwrap().contains("friendly"),
        "body should use default; got {:?}",
        v["body"]
    );
}

#[tokio::test]
async fn reload_picks_up_new_files() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let before: Vec<serde_json::Value> = serde_json::from_value(
        call(&runtime, "list", serde_json::json!({})).await.unwrap(),
    )
    .unwrap();
    assert!(
        !before.iter().any(|s| s["id"] == "tone"),
        "precondition: tone skill not yet on disk"
    );

    write_skill(forge.path(), "tone.skill.md", SKILL_TONE);

    let v = call(&runtime, "reload", serde_json::json!({}))
        .await
        .unwrap();
    let loaded = v["loaded"].as_u64().expect("loaded is a u64");
    assert!(loaded >= 1, "reload should load at least tone");

    let after: Vec<serde_json::Value> = serde_json::from_value(
        call(&runtime, "list", serde_json::json!({})).await.unwrap(),
    )
    .unwrap();
    assert!(after.iter().any(|s| s["id"] == "tone"));
}

#[tokio::test]
async fn unknown_skills_command_errors() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(&runtime, "not-a-thing", serde_json::json!({}))
        .await
        .unwrap_err();
    // Unknown commands never reach the handler — the loader surfaces
    // CommandNotFound directly.
    assert!(
        matches!(err, IpcError::CommandNotFound { .. }),
        "got {err:?}"
    );
}
