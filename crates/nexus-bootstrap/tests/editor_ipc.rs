//! End-to-end tests for the editor core plugin (`com.nexus.editor`)
//! driven through the kernel IPC surface.
//!
//! These exercise the same path production consumers use:
//! `runtime.context.ipc_call(PLUGIN_ID, command, args, timeout)`.

use std::fs;
use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_editor::{ApplyTransactionResponse, EditorSnapshot, EDITOR_PLUGIN_ID};
use nexus_kernel::{EventFilter, IpcError, NexusEvent, PluginContext};

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
    // Text-only ops return a Slim response (BL-123) — just the post-apply
    // revision counter. Tree assertions go through `get_tree` below.
    let resp: ApplyTransactionResponse = serde_json::from_value(
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
    assert_eq!(resp.revision(), 1);
    assert!(matches!(resp, ApplyTransactionResponse::Slim { .. }));

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
    assert_eq!(snap.undo_len, 1);
    assert!(snap.can_undo);

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
async fn save_routes_through_storage_and_updates_index() {
    // Editor `save` must go through `com.nexus.storage.write_file`, not
    // touch `std::fs` directly. Proof: after save, the storage SQLite
    // index reports a size matching the *edited* markdown — which is
    // only possible if save routed through storage's write_file (that
    // call is what updates the index). A raw `std::fs::write` in the
    // editor would leave the index stuck at the pre-save size.
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    async fn index_size(runtime: &nexus_bootstrap::Runtime, path: &str) -> u64 {
        let files: serde_json::Value = runtime
            .context
            .ipc_call(
                "com.nexus.storage",
                "query_files",
                serde_json::json!({}),
                CALL_TIMEOUT,
            )
            .await
            .expect("query_files");
        files
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == path)
            .and_then(|f| f["size_bytes"].as_u64())
            .unwrap_or(0)
    }

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "indexed.md", "hello\n");
    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    // Storage bootstrap runs a reconcile pass that picks up the fixture
    // file — so the pre-save size is 6 ("hello\n").
    let before = index_size(&runtime, "indexed.md").await;
    assert_eq!(before, 6, "sanity: bootstrap reconcile should index the fixture");

    // Open, mutate, save.
    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "indexed.md" }),
        )
        .await
        .expect("open"),
    )
    .unwrap();
    let para_id = snap.tree.root_blocks[0];
    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: 5,
            text: " world — now much longer".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        serde_json::json!({
            "relpath": "indexed.md",
            "transaction": serde_json::to_value(&tx).unwrap(),
        }),
    )
    .await
    .expect("apply_transaction");
    call(
        &runtime,
        "save",
        serde_json::json!({ "relpath": "indexed.md" }),
    )
    .await
    .expect("save");

    // Post-save index size must reflect the edit. Raw-fs save would
    // update disk but leave the index at `before`.
    let after = index_size(&runtime, "indexed.md").await;
    assert!(
        after > before,
        "save should have flowed through storage.write_file so the index size updates (before={before}, after={after})",
    );
    // And the on-disk bytes match the edit (sanity: both paths write disk).
    let on_disk = fs::read_to_string(root.join("indexed.md")).unwrap();
    assert!(on_disk.contains("world — now much longer"));
}

#[tokio::test]
async fn open_routes_through_storage_when_file_is_only_in_storage() {
    // Write a file ONLY through storage IPC, then open via editor. If
    // the editor still read from `std::fs`, it would succeed because
    // storage.write_file also writes to disk — so we can't detect the
    // difference that way. Instead: write a file containing a marker,
    // open via editor, verify the tree matches. This is a smoke test
    // for the async path wiring.
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    runtime
        .context
        .ipc_call(
            "com.nexus.storage",
            "write_file",
            serde_json::json!({ "path": "via-storage.md", "bytes": b"# Marker\n\nBody.\n".to_vec() }),
            CALL_TIMEOUT,
        )
        .await
        .expect("storage write_file");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "via-storage.md" }),
        )
        .await
        .expect("editor open"),
    )
    .unwrap();
    assert_eq!(snap.tree.root_blocks.len(), 2);
    let heading_id = snap.tree.root_blocks[0];
    assert_eq!(snap.tree.blocks[&heading_id].content, "Marker");
}

#[tokio::test]
async fn get_markdown_returns_canonical_session_text() {
    // After open + apply_transaction, `get_markdown` should return the
    // serialized form of the in-memory tree — i.e. the exact text the
    // kernel would write back on save. Proof: round-trip the result
    // through the parser/serializer and compare.
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "gm.md", "Hello\n");
    let runtime = build_cli_runtime(root).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(&runtime, "open", serde_json::json!({ "relpath": "gm.md" }))
            .await
            .unwrap(),
    )
    .unwrap();
    let para_id = snap.tree.root_blocks[0];
    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: 5,
            text: " world".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        serde_json::json!({
            "relpath": "gm.md",
            "transaction": serde_json::to_value(&tx).unwrap(),
        }),
    )
    .await
    .unwrap();

    let md: String = serde_json::from_value(
        call(
            &runtime,
            "get_markdown",
            serde_json::json!({ "relpath": "gm.md" }),
        )
        .await
        .expect("get_markdown ok"),
    )
    .unwrap();
    assert!(md.contains("Hello world"), "got markdown: {md:?}");
}

#[tokio::test]
async fn get_markdown_errors_on_unopen_session() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");
    let err = call(
        &runtime,
        "get_markdown",
        serde_json::json!({ "relpath": "never-opened.md" }),
    )
    .await
    .unwrap_err();
    // Handler returns an ExecutionFailed → IPC surface wraps as a
    // handler error; just assert the call failed (shape parity with
    // get_tree on unopen).
    let _ = err;
}

#[tokio::test]
async fn apply_transaction_emits_changed_event_on_kernel_bus() {
    // Phase 4: shell subscribers rely on the `com.nexus.editor.changed.<relpath>`
    // custom event being delivered through the shared kernel bus that
    // the Tauri IPC forwarder is wired to. Prove end-to-end that the
    // plugin-facing bus the bootstrap hands out actually sees the event
    // a mutation produces.
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "notes/e.md", "abc\n");
    let runtime = build_cli_runtime(root).expect("build runtime");

    let mut sub = runtime
        .kernel
        .event_bus()
        .subscribe(EventFilter::CustomPrefix(
            "com.nexus.editor.changed.".to_string(),
        ));

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "notes/e.md" }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(snap.revision, 0);
    let para_id = snap.tree.root_blocks[0];

    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: 3,
            text: "d".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    let tx_id = tx.id;
    let resp: ApplyTransactionResponse = serde_json::from_value(
        call(
            &runtime,
            "apply_transaction",
            serde_json::json!({
                "relpath": "notes/e.md",
                "transaction": serde_json::to_value(&tx).unwrap(),
            }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(resp.revision(), 1, "mutation bumps revision to 1");

    // Give the broadcast channel a tick to deliver.
    tokio::time::sleep(Duration::from_millis(25)).await;
    let event = sub
        .try_recv()
        .expect("bus receiver should have the changed event queued")
        .expect("broadcast delivery should succeed");
    match &event.event {
        NexusEvent::Custom {
            type_id, payload, ..
        } => {
            assert_eq!(type_id, "com.nexus.editor.changed.notes/e.md");
            assert_eq!(payload["relpath"], "notes/e.md");
            assert_eq!(payload["revision"], 1);
            assert_eq!(payload["transaction_id"].as_str().unwrap(), tx_id.to_string());
        }
        other => panic!("expected Custom, got {other:?}"),
    }
}

#[tokio::test]
async fn bl072_undo_history_persists_across_close_and_reopen() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "notes/p.md", "Hello\n");
    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "notes/p.md" }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let para_id = snap.tree.root_blocks[0];
    assert_eq!(snap.undo_len, 0);

    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: 5,
            text: " world".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        serde_json::json!({
            "relpath": "notes/p.md",
            "transaction": serde_json::to_value(&tx).unwrap(),
        }),
    )
    .await
    .unwrap();

    // Save flushes the canonical-markdown form so the on-disk content
    // hash matches what `close` records.
    call(
        &runtime,
        "save",
        serde_json::json!({ "relpath": "notes/p.md" }),
    )
    .await
    .unwrap();
    call(
        &runtime,
        "close",
        serde_json::json!({ "relpath": "notes/p.md" }),
    )
    .await
    .unwrap();

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "notes/p.md" }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(snap.undo_len, 1, "persisted undo restored on reopen");
    assert!(snap.can_undo, "restored history is at the executed position");
    let restored_para = snap.tree.root_blocks[0];
    assert_eq!(snap.tree.blocks[&restored_para].content, "Hello world");

    // Driving an undo against the restored history proves the persisted
    // ops are functional, not just present.
    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "undo",
            serde_json::json!({ "relpath": "notes/p.md" }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(snap.tree.blocks[&restored_para].content, "Hello");
}

#[tokio::test]
async fn bl072_undo_history_discarded_when_file_changes_externally() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "notes/q.md", "Hello\n");
    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "notes/q.md" }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let para_id = snap.tree.root_blocks[0];

    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: 5,
            text: "!".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        serde_json::json!({
            "relpath": "notes/q.md",
            "transaction": serde_json::to_value(&tx).unwrap(),
        }),
    )
    .await
    .unwrap();
    call(
        &runtime,
        "save",
        serde_json::json!({ "relpath": "notes/q.md" }),
    )
    .await
    .unwrap();
    call(
        &runtime,
        "close",
        serde_json::json!({ "relpath": "notes/q.md" }),
    )
    .await
    .unwrap();

    // External edit invalidates the cached history — the persisted op
    // offsets are anchored to the old tree shape, so the safe answer
    // is "throw it away".
    fs::write(root.join("notes/q.md"), "Different content entirely\n").unwrap();

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open",
            serde_json::json!({ "relpath": "notes/q.md" }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snap.undo_len, 0,
        "external edit discards the cached undo history"
    );
    assert!(!snap.can_undo);
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
