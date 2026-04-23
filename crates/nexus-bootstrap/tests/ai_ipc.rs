//! End-to-end tests for the AI core plugin (`com.nexus.ai`) driven
//! through the kernel IPC surface.
//!
//! Pins the contract the ChatPanel (session list/load/save/delete) and
//! status widgets depend on. Provider-backed handlers
//! (`ask`, `stream_chat`, `stream_ask`, `index_file`) are intentionally
//! NOT exercised here — they make real network calls against a
//! configured LLM provider, which isn't available in CI. Those are
//! covered by provider-level unit tests inside `nexus-ai`.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{IpcError, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const AI_PLUGIN_ID: &str = "com.nexus.ai";

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
        .ipc_call(AI_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn config_returns_shape_with_ai_and_embedding_slots() {
    // `config` is the one fully-sync handler — no provider or storage
    // I/O. Must round-trip through IPC and return an object with `ai`
    // and `embedding` keys (either of which may be null when no
    // provider is configured in the environment).
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "config", serde_json::json!({}))
        .await
        .expect("config ok");
    assert!(v.is_object(), "config should return an object: {v:?}");
    assert!(v.get("ai").is_some(), "config missing `ai` key: {v:?}");
    assert!(
        v.get("embedding").is_some(),
        "config missing `embedding` key: {v:?}"
    );
}

#[tokio::test]
async fn status_reports_vectorstore_count_through_storage() {
    // `status` routes through `com.nexus.storage` (vectorstore_count)
    // via the plugin's wired KernelPluginContext. On a fresh forge
    // the vector store is empty.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "status", serde_json::json!({}))
        .await
        .expect("status ok");
    assert!(v.is_object(), "status should return an object: {v:?}");
    assert_eq!(
        v["indexed_chunks"].as_u64(),
        Some(0),
        "fresh forge should report zero indexed chunks"
    );
    // provider fields are null-or-string; just assert the keys exist.
    assert!(v.get("ai_provider").is_some());
    assert!(v.get("embedding_provider").is_some());
}

#[tokio::test]
async fn vectorstore_count_matches_status_on_empty_forge() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(&runtime, "vectorstore_count", serde_json::json!({}))
        .await
        .expect("vectorstore_count ok");
    assert_eq!(v["count"].as_u64(), Some(0));
}

#[tokio::test]
async fn session_save_creates_sessions_dir_on_fresh_forge() {
    // Regression: session_save used to call plain `ctx.write_file`,
    // which is tokio::fs::write with no parent-dir creation. On a
    // fresh forge `.forge/chat/sessions/` doesn't exist yet, so the
    // first save would silently fail. Fix routes through
    // `com.nexus.storage::write_file`, whose atomic_write helper
    // runs `create_dir_all` on the parent.
    let forge = scratch_forge();
    let sessions_dir = forge.path().join(".forge").join("chat").join("sessions");
    assert!(!sessions_dir.exists(), "precondition: sessions dir absent");

    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    call(
        &runtime,
        "session_save",
        serde_json::json!({ "id": "first-ever", "messages": [] }),
    )
    .await
    .expect("session_save on fresh forge");

    assert!(sessions_dir.exists(), "sessions dir created");
    assert!(sessions_dir.join("first-ever.json").exists(), "file written");
}

#[tokio::test]
async fn session_save_then_load_roundtrips_payload() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let payload = serde_json::json!({
        "id": "smoke-session-1",
        "title": "Smoke",
        "messages": [
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": "hello" },
        ],
        "updated_at": "2026-04-23T00:00:00Z",
    });
    let saved = call(&runtime, "session_save", payload.clone())
        .await
        .expect("session_save ok");
    assert_eq!(saved["id"], "smoke-session-1");
    assert!(saved["bytes"].as_u64().unwrap_or(0) > 0);

    let loaded = call(
        &runtime,
        "session_load",
        serde_json::json!({ "id": "smoke-session-1" }),
    )
    .await
    .expect("session_load ok");
    assert_eq!(loaded["title"], "Smoke");
    assert_eq!(loaded["messages"][0]["content"], "hi");
}

#[tokio::test]
async fn session_list_includes_saved_session_and_delete_removes_it() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Empty forge → empty list.
    let before = call(&runtime, "session_list", serde_json::json!({}))
        .await
        .expect("session_list ok");
    assert_eq!(before.as_array().map(Vec::len), Some(0));

    call(
        &runtime,
        "session_save",
        serde_json::json!({
            "id": "list-me",
            "title": "List me",
            "updated_at": "2026-04-23T00:00:00Z",
            "messages": [],
        }),
    )
    .await
    .expect("session_save ok");

    let list = call(&runtime, "session_list", serde_json::json!({}))
        .await
        .expect("session_list ok");
    let entries = list.as_array().expect("array");
    assert!(
        entries.iter().any(|e| e["id"] == "list-me"),
        "expected saved session in list; got {entries:?}"
    );

    let del = call(
        &runtime,
        "session_delete",
        serde_json::json!({ "id": "list-me" }),
    )
    .await
    .expect("session_delete ok");
    assert_eq!(del["deleted"], true);

    let after = call(&runtime, "session_list", serde_json::json!({}))
        .await
        .expect("session_list after-delete ok");
    assert!(
        !after
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == "list-me"),
        "session should be gone after delete"
    );
}

#[tokio::test]
async fn session_load_missing_returns_null_not_error() {
    // Fresh forge reading an unsaved id returns JSON null — keeps the UI
    // quiet on first boot.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let v = call(
        &runtime,
        "session_load",
        serde_json::json!({ "id": "never-saved" }),
    )
    .await
    .expect("session_load should succeed for missing id");
    assert!(v.is_null(), "expected null for missing session; got {v:?}");
}

#[tokio::test]
async fn session_save_rejects_path_traversal_ids() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(
        &runtime,
        "session_save",
        serde_json::json!({ "id": "../escape", "messages": [] }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn unknown_ai_command_returns_command_not_found() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    let err = call(&runtime, "no-such-ai-command", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == AI_PLUGIN_ID && command == "no-such-ai-command"
        ),
        "got {err:?}"
    );
}
