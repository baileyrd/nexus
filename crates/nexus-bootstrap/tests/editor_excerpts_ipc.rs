//! BL-141 Phase 1 — end-to-end tests for `com.nexus.editor::open_excerpts`.
//!
//! Exercises the synthetic-session construction path, the
//! storage-IPC-backed source-file reads, the overlapping-range merge
//! logic, and the read-only guards that BL-141 Phase 1 ships in
//! place of full read-write multibuffer routing (Phase 2 follow-up).

use std::fs;
use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_editor::{EditorSnapshot, EDITOR_PLUGIN_ID};
use nexus_kernel::{IpcError, PluginContext};
use serde_json::json;

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
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

/// Helper: pull the `kind` discriminant out of a serialized BlockType
/// JSON value. BlockType serializes as
/// `{ "kind": "<variant_snake_case>", ...payload }`.
fn block_kind(v: &serde_json::Value) -> &str {
    v.get("kind").and_then(|k| k.as_str()).unwrap_or("?")
}

/// 1) Construction — single excerpt across one file returns a
///    synthetic session whose root block is an `Excerpt` with the
///    requested metadata + snapshot content.
#[tokio::test]
async fn open_excerpts_constructs_synthetic_session_with_excerpt_block() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "src/lib.md", "alpha\nbeta\ngamma\ndelta\nepsilon\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "src/lib.md", "line_start": 2, "line_end": 4, "label": "context" }
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();

    assert!(
        snap.relpath.starts_with("multibuffer://"),
        "expected synthetic relpath, got '{}'",
        snap.relpath
    );
    assert_eq!(snap.tree.root_blocks.len(), 1);

    let block_id = snap.tree.root_blocks[0];
    let block = &snap.tree.blocks[&block_id];
    let ty = serde_json::to_value(&block.ty).unwrap();
    assert_eq!(block_kind(&ty), "excerpt");
    assert_eq!(ty["source_relpath"], "src/lib.md");
    assert_eq!(ty["line_start"], 2);
    assert_eq!(ty["line_end"], 4);
    assert_eq!(ty["label"], "context");
    assert_eq!(block.content, "beta\ngamma\ndelta");
}

/// 2) Multi-file reads — excerpts from two different source files
///    are each rendered as their own block, in input order.
#[tokio::test]
async fn open_excerpts_reads_multiple_source_files() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "a.md", "a-one\na-two\na-three\n");
    write_note(&root, "b.md", "b-one\nb-two\nb-three\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "a.md", "line_start": 1, "line_end": 1 },
                    { "relpath": "b.md", "line_start": 2, "line_end": 3 },
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();

    assert_eq!(snap.tree.root_blocks.len(), 2);
    let first = &snap.tree.blocks[&snap.tree.root_blocks[0]];
    let second = &snap.tree.blocks[&snap.tree.root_blocks[1]];
    let first_ty = serde_json::to_value(&first.ty).unwrap();
    let second_ty = serde_json::to_value(&second.ty).unwrap();
    assert_eq!(first_ty["source_relpath"], "a.md");
    assert_eq!(first.content, "a-one");
    assert_eq!(second_ty["source_relpath"], "b.md");
    assert_eq!(second.content, "b-two\nb-three");
}

/// 3) Empty items list — rejected with a parseable error message
///    (Phase 1 contract: a multibuffer with zero excerpts is
///    nonsense, surface it immediately rather than handing back an
///    unusable empty session).
#[tokio::test]
async fn open_excerpts_rejects_empty_items() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    let runtime = build_cli_runtime(root).expect("build runtime");

    let err = call(&runtime, "open_excerpts", json!({ "items": [] }))
        .await
        .expect_err("empty items must error");
    assert!(
        err.to_string().contains("non-empty"),
        "expected 'non-empty' in error, got: {err}"
    );
}

/// 4) Overlapping ranges in the same file are merged into a single
///    excerpt block. Adjacent ranges (touching at line N+1) merge too.
///    Ranges from a different file remain independent.
#[tokio::test]
async fn open_excerpts_merges_overlapping_ranges_within_a_file() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(
        &root,
        "long.md",
        "L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\n",
    );
    write_note(&root, "other.md", "O1\nO2\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "long.md",  "line_start": 1,  "line_end": 3 },
                    { "relpath": "long.md",  "line_start": 3,  "line_end": 5  }, // overlaps with above
                    { "relpath": "long.md",  "line_start": 6,  "line_end": 7  }, // adjacent to above (6 = 5+1)
                    { "relpath": "other.md", "line_start": 1,  "line_end": 1  },
                    { "relpath": "long.md",  "line_start": 10, "line_end": 10 }, // disjoint from earlier
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();

    // long.md ranges 1-3, 3-5, 6-7 merge into a single 1-7 excerpt.
    // long.md range 10-10 stays separate. other.md is its own block.
    assert_eq!(snap.tree.root_blocks.len(), 3);

    let blocks: Vec<_> = snap
        .tree
        .root_blocks
        .iter()
        .map(|id| {
            let b = &snap.tree.blocks[id];
            let ty = serde_json::to_value(&b.ty).unwrap();
            (
                ty["source_relpath"].as_str().unwrap().to_string(),
                ty["line_start"].as_u64().unwrap(),
                ty["line_end"].as_u64().unwrap(),
                b.content.clone(),
            )
        })
        .collect();
    assert_eq!(blocks[0].0, "long.md");
    assert_eq!(blocks[0].1, 1);
    assert_eq!(blocks[0].2, 7);
    assert_eq!(blocks[0].3, "L1\nL2\nL3\nL4\nL5\nL6\nL7");
    assert_eq!(blocks[1].0, "other.md");
    assert_eq!(blocks[2].0, "long.md");
    assert_eq!(blocks[2].1, 10);
    assert_eq!(blocks[2].2, 10);
    assert_eq!(blocks[2].3, "L10");
}

/// 5) Phase 2 — non-content ops (InsertText / DeleteText / structural)
///    against the synthetic session still error with the BL-141
///    Approach A semantics. The source file's bytes stay untouched
///    when an op is rejected.
#[tokio::test]
async fn open_excerpts_session_rejects_non_content_ops() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    let original = "alpha\nbeta\ngamma\n";
    write_note(&root, "doc.md", original);

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "doc.md", "line_start": 1, "line_end": 2 }
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();
    let block_id = snap.tree.root_blocks[0];

    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id,
            pos: 0,
            text: "X".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    let err = call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snap.relpath, "transaction": tx }),
    )
    .await
    .expect_err("InsertText must be rejected on a multibuffer");
    assert!(
        err.to_string().contains("multibuffer"),
        "expected multibuffer-related error, got: {err}"
    );

    let on_disk = fs::read_to_string(root.join("doc.md")).unwrap();
    assert_eq!(on_disk, original, "source file must be untouched");
}

/// BL-141 Phase 2 — `UpdateBlockContent` on an Excerpt block updates
/// the in-memory snapshot AND a subsequent `save` splices the new
/// content into the source file's line range, preserving the lines
/// outside the range.
#[tokio::test]
async fn open_excerpts_update_block_content_round_trips_to_source_on_save() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    let original = "header line\nbody line 1\nbody line 2\nfooter line\n";
    write_note(&root, "doc.md", original);

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "doc.md", "line_start": 2, "line_end": 3 }
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();
    let block_id = snap.tree.root_blocks[0];
    let snapshot_relpath = snap.relpath.clone();
    let starting_content = snap.tree.blocks[&block_id].content.clone();
    assert_eq!(starting_content, "body line 1\nbody line 2");

    // Edit: replace the excerpt's snapshot with a 3-line block.
    let tx = Transaction::new(
        vec![Operation::UpdateBlockContent {
            id: block_id,
            old_content: starting_content,
            new_content: "edited line A\nedited line B\nedited line C".into(),
            old_annotations: Vec::new(),
            new_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snapshot_relpath, "transaction": tx }),
    )
    .await
    .expect("apply_transaction (UpdateBlockContent) must succeed");

    call(
        &runtime,
        "save",
        json!({ "relpath": snapshot_relpath }),
    )
    .await
    .expect("save must succeed for synthetic session in Phase 2");

    let on_disk = fs::read_to_string(root.join("doc.md")).unwrap();
    assert_eq!(
        on_disk, "header line\nedited line A\nedited line B\nedited line C\nfooter line\n",
        "header + footer preserved; excerpt range replaced with edited content"
    );
}

/// BL-141 Phase 2 — two non-overlapping excerpts from the same
/// source file both round-trip on save. Reverse-line-order
/// processing inside `splice_excerpts` keeps the later range valid
/// even when the earlier splice shifts line counts.
#[tokio::test]
async fn open_excerpts_multi_excerpt_same_file_save_handles_shifts() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    // 10 lines so the two excerpts are clearly non-overlapping.
    let original = "L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\n";
    write_note(&root, "big.md", original);

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "big.md", "line_start": 2, "line_end": 3 },
                    { "relpath": "big.md", "line_start": 7, "line_end": 8 },
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();
    assert_eq!(snap.tree.root_blocks.len(), 2);
    let first_id = snap.tree.root_blocks[0];
    let second_id = snap.tree.root_blocks[1];
    let snapshot_relpath = snap.relpath.clone();

    // Edit the first excerpt (lines 2-3) to grow to 4 lines; edit
    // the second (lines 7-8) to shrink to 1 line. Both saves must
    // land correctly even though the first changes the line count
    // before the second.
    let tx1 = Transaction::new(
        vec![Operation::UpdateBlockContent {
            id: first_id,
            old_content: "L2\nL3".into(),
            new_content: "L2-A\nL2-B\nL2-C\nL2-D".into(),
            old_annotations: Vec::new(),
            new_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snapshot_relpath, "transaction": tx1 }),
    )
    .await
    .expect("first UpdateBlockContent");

    let tx2 = Transaction::new(
        vec![Operation::UpdateBlockContent {
            id: second_id,
            old_content: "L7\nL8".into(),
            new_content: "L7+L8".into(),
            old_annotations: Vec::new(),
            new_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snapshot_relpath, "transaction": tx2 }),
    )
    .await
    .expect("second UpdateBlockContent");

    call(
        &runtime,
        "save",
        json!({ "relpath": snapshot_relpath }),
    )
    .await
    .expect("save");

    let on_disk = fs::read_to_string(root.join("big.md")).unwrap();
    assert_eq!(
        on_disk,
        "L1\nL2-A\nL2-B\nL2-C\nL2-D\nL4\nL5\nL6\nL7+L8\nL9\nL10\n",
        "both splices land correctly with line-range shifting; unedited lines preserved"
    );
}

/// BL-141 Phase 2 — save against a multibuffer with no edits is a
/// no-op writeback (each source file is rewritten with its own
/// content, since the splice content equals the original captured
/// snapshot). Verifies the splice path doesn't corrupt the file
/// even when nothing changed.
#[tokio::test]
async fn open_excerpts_save_with_no_edits_preserves_source_file() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    let original = "one\ntwo\nthree\nfour\nfive\n";
    write_note(&root, "doc.md", original);

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "doc.md", "line_start": 2, "line_end": 4 }
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();
    let snapshot_relpath = snap.relpath.clone();

    call(
        &runtime,
        "save",
        json!({ "relpath": snapshot_relpath }),
    )
    .await
    .expect("save no-op must succeed");

    let on_disk = fs::read_to_string(root.join("doc.md")).unwrap();
    assert_eq!(on_disk, original, "source file must be byte-identical");
}

/// 6) Excerpt metadata — each rendered block carries the
///    `source_relpath / line_start / line_end / label` payload so the
///    shell can render the file-path + line-range header. Multiple
///    excerpts from the same file preserve their distinct labels.
#[tokio::test]
async fn open_excerpts_preserves_per_excerpt_labels() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "diag.md", "first\nsecond\nthird\nfourth\nfifth\n");
    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "diag.md", "line_start": 1, "line_end": 1, "label": "error: missing semicolon" },
                    { "relpath": "diag.md", "line_start": 4, "line_end": 4, "label": "warning: unused variable" },
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();

    assert_eq!(snap.tree.root_blocks.len(), 2);
    let ty0 = serde_json::to_value(&snap.tree.blocks[&snap.tree.root_blocks[0]].ty).unwrap();
    let ty1 = serde_json::to_value(&snap.tree.blocks[&snap.tree.root_blocks[1]].ty).unwrap();
    assert_eq!(ty0["label"], "error: missing semicolon");
    assert_eq!(ty1["label"], "warning: unused variable");
}
