//! End-to-end tests for the database plugin's IPC surface.
//!
//! Proves that `com.nexus.database`'s `csv_import`, `csv_export`, and
//! `formula_eval` handlers round-trip correctly through the kernel, so
//! invokers (CLI / TUI) can reach the pure-logic helpers without a direct
//! `nexus-database` dependency.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{IpcError, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const DATABASE_PLUGIN_ID: &str = "com.nexus.database";

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
        .ipc_call(DATABASE_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn csv_import_parses_records_through_ipc() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let csv = b"name,score\nAlice,95\nBob,87\n".to_vec();
    let value = call(
        &runtime,
        "csv_import",
        serde_json::json!({
            "csv_bytes": csv,
            "field_names": ["name", "score"],
            "has_header": true,
        }),
    )
    .await
    .expect("csv_import dispatches cleanly");

    assert_eq!(value["imported"], 2);
    assert_eq!(value["skipped"], 0);
    let records = value["records"].as_array().expect("records array");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["name"], "Alice");
    assert_eq!(records[1]["score"], 87.0);
}

#[tokio::test]
async fn csv_export_roundtrips_records_through_ipc() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let records = serde_json::json!([
        { "id": "r1", "name": "Alice", "score": 95 },
        { "id": "r2", "name": "Bob", "score": 87 },
    ]);
    let value = call(
        &runtime,
        "csv_export",
        serde_json::json!({
            "records": records,
            "field_names": ["name", "score"],
        }),
    )
    .await
    .expect("csv_export dispatches cleanly");

    assert_eq!(value["count"], 2);
    let bytes: Vec<u8> = serde_json::from_value(value["csv_bytes"].clone())
        .expect("csv_bytes is Vec<u8>");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("name,score"), "got: {text}");
    assert!(text.contains("Alice,95"), "got: {text}");
    assert!(text.contains("Bob,87"), "got: {text}");
}

#[tokio::test]
async fn formula_eval_computes_over_record_fields_through_ipc() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let value = call(
        &runtime,
        "formula_eval",
        serde_json::json!({
            "expression": "prop(\"score\") + 5",
            "fields": { "score": 10 },
        }),
    )
    .await
    .expect("formula_eval dispatches cleanly");

    assert_eq!(value["display"], "15");
}

#[tokio::test]
async fn apply_view_groups_records_by_status_field() {
    // The only non-CSV/formula handler on `com.nexus.database` is
    // `apply_view`, which runs the pure filter/sort/group pipeline
    // in-memory. Base-table CRUD (create_table / insert_record / query
    // / update_cell / delete) is intentionally NOT on this plugin —
    // those go through `com.nexus.storage` (`base_create`,
    // `base_record_create`, `base_list`, `base_record_update`,
    // `base_record_delete`) which owns the forge SQLite. See
    // ARCHITECTURE.md §4.2. Those handlers are covered via
    // storage-focused tests elsewhere.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let v = call(
        &runtime,
        "apply_view",
        serde_json::json!({
            "records": [
                { "id": "a", "status": "todo", "priority": 1 },
                { "id": "b", "status": "done", "priority": 2 },
                { "id": "c", "status": "todo", "priority": 3 },
            ],
            "schema": { "version": "1.0", "fields": {} },
            "view": {
                "name": "Board",
                "type": "kanban",
                "fields": ["title"],
                "sort": [{ "field": "priority", "direction": "asc" }],
                "filter": [],
                "groupField": "status",
            },
        }),
    )
    .await
    .expect("apply_view dispatches cleanly");
    assert_eq!(v["layout"]["kind"], "grouped");
    let groups = v["layout"]["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 2);
}

#[tokio::test]
async fn csv_import_with_malformed_args_returns_handler_error() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let err = call(
        &runtime,
        "csv_import",
        serde_json::json!({ "csv_bytes": "not-a-byte-array" }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, IpcError::PluginCrashedDuringCall { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn unknown_database_command_returns_command_not_found() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let err = call(&runtime, "not-a-real-command", serde_json::json!({}))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == DATABASE_PLUGIN_ID && command == "not-a-real-command"
        ),
        "got {err:?}"
    );
}
