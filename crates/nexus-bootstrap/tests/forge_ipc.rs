//! End-to-end tests for the storage plugin's forge-tree IPC handlers.
//!
//! These exercise the same path the Tauri shell's `forge.rs` commands use:
//! `runtime.context.ipc_call("com.nexus.storage", <command>, args, timeout)`.
//!
//! The handlers covered here — `list_dir`, `create_file`, `create_dir`,
//! `rename_entry`, `delete_entry` — are the file-tree CRUD surface. The
//! shell has no direct `std::fs` path for these; all I/O goes through the
//! storage plugin.

use std::fs;
use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{IpcError, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";

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
        .ipc_call(STORAGE_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn list_dir_returns_sorted_entries_and_hides_forge_dir() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    fs::write(root.join("notes/b.md"), b"b").unwrap();
    fs::write(root.join("notes/a.md"), b"a").unwrap();
    fs::create_dir_all(root.join("notes/sub")).unwrap();
    let runtime = build_cli_runtime(root).expect("build runtime");

    // Root listing must not include `.forge/`.
    let entries = call(&runtime, "list_dir", serde_json::json!({ "relpath": "" }))
        .await
        .expect("list_dir root");
    let names: Vec<String> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.iter().any(|n| n == ".forge"), "got {names:?}");
    assert!(names.contains(&"notes".to_string()));

    // Subdir listing: dirs first, then files, each alphabetically.
    let entries = call(
        &runtime,
        "list_dir",
        serde_json::json!({ "relpath": "notes" }),
    )
    .await
    .expect("list_dir notes");
    let arr = entries.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["name"], "sub");
    assert_eq!(arr[0]["isDir"], true);
    assert_eq!(arr[1]["name"], "a.md");
    assert_eq!(arr[2]["name"], "b.md");
    assert_eq!(arr[1]["relpath"], "notes/a.md");
}

#[tokio::test]
async fn create_file_creates_empty_file_and_rejects_overwrite() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    call(
        &runtime,
        "create_file",
        serde_json::json!({ "relpath": "notes/new.md" }),
    )
    .await
    .expect("create_file ok");
    let meta = fs::metadata(forge.path().join("notes/new.md")).unwrap();
    assert!(meta.is_file());
    assert_eq!(meta.len(), 0);

    // Second call on the same path is rejected.
    let err = call(
        &runtime,
        "create_file",
        serde_json::json!({ "relpath": "notes/new.md" }),
    )
    .await
    .expect_err("second create_file must fail");
    match err {
        IpcError::PluginCrashedDuringCall { .. } => {}
        other => panic!("expected PluginCrashedDuringCall, got {other:?}"),
    }
}

#[tokio::test]
async fn create_dir_creates_and_rejects_existing() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    call(
        &runtime,
        "create_dir",
        serde_json::json!({ "relpath": "notes/folder" }),
    )
    .await
    .expect("create_dir ok");
    assert!(forge.path().join("notes/folder").is_dir());

    let err = call(
        &runtime,
        "create_dir",
        serde_json::json!({ "relpath": "notes/folder" }),
    )
    .await
    .expect_err("second create_dir must fail");
    match err {
        IpcError::PluginCrashedDuringCall { .. } => {}
        other => panic!("expected PluginCrashedDuringCall, got {other:?}"),
    }
}

#[tokio::test]
async fn rename_entry_moves_file_and_updates_index() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    // Write via IPC so the file lands in the SQLite index.
    call(
        &runtime,
        "write_file",
        serde_json::json!({ "path": "notes/old.md", "bytes": b"# old\n".to_vec() }),
    )
    .await
    .expect("write_file");

    // Rename.
    call(
        &runtime,
        "rename_entry",
        serde_json::json!({ "from": "notes/old.md", "to": "notes/new.md" }),
    )
    .await
    .expect("rename_entry");

    assert!(!root.join("notes/old.md").exists());
    assert!(root.join("notes/new.md").exists());

    // Index rows followed the rename.
    let exists_new: serde_json::Value = call(
        &runtime,
        "file_exists",
        serde_json::json!({ "path": "notes/new.md" }),
    )
    .await
    .expect("file_exists new");
    assert_eq!(exists_new["exists"], true);

    let exists_old: serde_json::Value = call(
        &runtime,
        "file_exists",
        serde_json::json!({ "path": "notes/old.md" }),
    )
    .await
    .expect("file_exists old");
    assert_eq!(exists_old["exists"], false);
}

#[tokio::test]
async fn rename_entry_rejects_existing_destination() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    fs::write(forge.path().join("notes/a.md"), b"a").unwrap();
    fs::write(forge.path().join("notes/b.md"), b"b").unwrap();

    let err = call(
        &runtime,
        "rename_entry",
        serde_json::json!({ "from": "notes/a.md", "to": "notes/b.md" }),
    )
    .await
    .expect_err("rename onto existing must fail");
    match err {
        IpcError::PluginCrashedDuringCall { .. } => {}
        other => panic!("expected PluginCrashedDuringCall, got {other:?}"),
    }
}

#[tokio::test]
async fn delete_entry_handles_files_and_directories() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    // File path.
    fs::write(root.join("notes/gone.md"), b"x").unwrap();
    call(
        &runtime,
        "delete_entry",
        serde_json::json!({ "relpath": "notes/gone.md" }),
    )
    .await
    .expect("delete_entry file");
    assert!(!root.join("notes/gone.md").exists());

    // Directory path (non-empty).
    fs::create_dir_all(root.join("notes/dir/nested")).unwrap();
    fs::write(root.join("notes/dir/a.md"), b"a").unwrap();
    fs::write(root.join("notes/dir/nested/b.md"), b"b").unwrap();
    call(
        &runtime,
        "delete_entry",
        serde_json::json!({ "relpath": "notes/dir" }),
    )
    .await
    .expect("delete_entry dir");
    assert!(!root.join("notes/dir").exists());
}

#[tokio::test]
async fn path_confinement_rejects_traversal() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    for cmd in ["list_dir", "create_file", "create_dir", "delete_entry"] {
        let err = call(
            &runtime,
            cmd,
            serde_json::json!({ "relpath": "../escaped" }),
        )
        .await
        .expect_err(&format!("{cmd} with ..  must fail"));
        match err {
            IpcError::PluginCrashedDuringCall { .. } => {}
            other => panic!("{cmd}: expected PluginCrashedDuringCall, got {other:?}"),
        }
    }
}
