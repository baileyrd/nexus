//! End-to-end tests for the editor core plugin (`com.nexus.editor`)
//! driven through the kernel IPC surface.
//!
//! These exercise the same path production consumers use:
//! `runtime.context.ipc_call(PLUGIN_ID, command, args, timeout)`.

use std::fs;
use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_editor::{EditorSnapshot, EDITOR_PLUGIN_ID};
use nexus_kernel::{IpcError, PluginContext};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    // Storage plugin's on_init opens a StorageEngine — we reuse the same
    // forge init the other tests do so bootstrap succeeds.
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

fn write_note(root: &std::path::Path, relpath: &str, body: &str) {
    let abs = root.join(relpath);
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
        .ipc_call(EDITOR_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn open_apply_transaction_get_tree_save_roundtrip() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "note.md", "Hello\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    // open → snapshot with one paragraph
    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "note.md" }),
        )
        .await
        .expect("open ok"),
    )
    .unwrap();
    let para_id = snap.tree.root_blocks[0];
    assert_eq!(snap.tree.blocks[&para_id].content, "Hello");

    // apply_transaction → append " world"
    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: 5,
            text: " world".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "apply_transaction",
            serde_json::json!({
                "relpath": "note.md",
                "transaction": serde_json::to_value(&tx).unwrap(),
            }),
        )
        .await
        .expect("apply_transaction ok"),
    )
    .unwrap();
    assert_eq!(snap.tree.blocks[&para_id].content, "Hello world");
    assert_eq!(snap.undo_len, 1);
    assert!(snap.can_undo);

    // get_tree returns the same state
    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "get_tree",
            serde_json::json!({ "relpath": "note.md" }),
        )
        .await
        .expect("get_tree ok"),
    )
    .unwrap();
    assert_eq!(snap.tree.blocks[&para_id].content, "Hello world");

    // save writes back to disk
    call(
        &runtime,
        "save",
        serde_json::json!({ "relpath": "note.md" }),
    )
    .await
    .expect("save ok");
    let on_disk = fs::read_to_string(root.join("note.md")).unwrap();
    assert!(on_disk.contains("Hello world"));
}

#[tokio::test]
async fn undo_reverses_apply_transaction() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "n.md", "abc\n");
    let runtime = build_cli_runtime(root).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(&runtime, "open", serde_json::json!({ "relpath": "n.md" }))
            .await
            .unwrap(),
    )
    .unwrap();
    let para_id = snap.tree.root_blocks[0];

    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: 3,
            text: "XYZ".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        serde_json::json!({ "relpath": "n.md", "transaction": serde_json::to_value(&tx).unwrap() }),
    )
    .await
    .unwrap();

    let snap: EditorSnapshot = serde_json::from_value(
        call(&runtime, "undo", serde_json::json!({ "relpath": "n.md" }))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snap.tree.blocks[&para_id].content, "abc");
    assert!(!snap.can_undo);
    assert!(snap.can_redo);
}

#[tokio::test]
async fn list_open_tracks_sessions() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "a.md", "a\n");
    write_note(&root, "b.md", "b\n");
    let runtime = build_cli_runtime(root).expect("build runtime");

    let open: Vec<String> = serde_json::from_value(
        call(&runtime, "list_open", serde_json::json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(open.is_empty());

    call(&runtime, "open", serde_json::json!({ "relpath": "a.md" }))
        .await
        .unwrap();
    call(&runtime, "open", serde_json::json!({ "relpath": "b.md" }))
        .await
        .unwrap();

    let open: Vec<String> = serde_json::from_value(
        call(&runtime, "list_open", serde_json::json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(open, vec!["a.md".to_string(), "b.md".into()]);

    call(&runtime, "close", serde_json::json!({ "relpath": "a.md" }))
        .await
        .unwrap();
    let open: Vec<String> = serde_json::from_value(
        call(&runtime, "list_open", serde_json::json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(open, vec!["b.md".to_string()]);
}

#[tokio::test]
async fn unknown_editor_command_returns_command_not_found() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let err = call(&runtime, "no-such-command", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == EDITOR_PLUGIN_ID && command == "no-such-command"
        ),
        "got {err:?}"
    );
}
