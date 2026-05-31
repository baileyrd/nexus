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
use nexus_kernel::{Ipc as _, IpcError};
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

/// 5a) Phase 2 Approach B — `InsertText` and `DeleteText` on Excerpt
///     blocks succeed and mutate the in-memory snapshot. The source
///     file's bytes stay untouched until `save` runs (covered by 6).
///     This replaced the Approach-A rejection assertion: per-keystroke
///     ops are no longer the "commit-via-UpdateBlockContent" detour.
#[tokio::test]
async fn open_excerpts_session_accepts_text_ops_on_excerpt_blocks() {
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
    let starting = snap.tree.blocks[&block_id].content.clone();
    assert_eq!(starting, "alpha\nbeta");

    // InsertText at position 0 inserts at the start of the excerpt.
    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id,
            pos: 0,
            text: "X".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snap.relpath, "transaction": tx }),
    )
    .await
    .expect("InsertText must succeed on an Excerpt block");

    // Re-read the snapshot to confirm the excerpt content was mutated.
    let after: EditorSnapshot = serde_json::from_value(
        call(&runtime, "get_tree", json!({ "relpath": snap.relpath }))
            .await
            .expect("get_tree ok"),
    )
    .unwrap();
    assert_eq!(after.tree.blocks[&block_id].content, "Xalpha\nbeta");

    // DeleteText removes the inserted character.
    let tx = Transaction::new(
        vec![Operation::DeleteText {
            block_id,
            pos: 0,
            deleted_text: "X".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snap.relpath, "transaction": tx }),
    )
    .await
    .expect("DeleteText must succeed on an Excerpt block");

    let after: EditorSnapshot = serde_json::from_value(
        call(&runtime, "get_tree", json!({ "relpath": snap.relpath }))
            .await
            .expect("get_tree ok"),
    )
    .unwrap();
    assert_eq!(after.tree.blocks[&block_id].content, "alpha\nbeta");

    // The on-disk source file is untouched until `save` runs.
    let on_disk = fs::read_to_string(root.join("doc.md")).unwrap();
    assert_eq!(on_disk, original, "source file must be untouched pre-save");
}

/// 5b) Phase 2 Approach B — structural ops (`InsertBlock` /
///     `DeleteBlock` / `ReparentBlock` / `UpdateAnnotations`) on the
///     synthetic session still error: they don't have a line-range
///     splice mapping and would corrupt the synthetic tree's
///     Excerpt-only invariant.
#[tokio::test]
async fn open_excerpts_session_rejects_structural_ops() {
    use nexus_editor::{Block, BlockType, Operation, Transaction, TransactionMetadata};

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

    // InsertBlock — attempt to add a non-Excerpt sibling.
    let tx = Transaction::new(
        vec![Operation::InsertBlock {
            block: Block::new(BlockType::Paragraph).with_content("interloper"),
            parent_id: None,
            index_in_parent: 0,
        }],
        TransactionMetadata::default(),
    );
    let err = call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snap.relpath, "transaction": tx }),
    )
    .await
    .expect_err("InsertBlock must be rejected on a multibuffer");
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

    call(&runtime, "save", json!({ "relpath": snapshot_relpath }))
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

    call(&runtime, "save", json!({ "relpath": snapshot_relpath }))
        .await
        .expect("save");

    let on_disk = fs::read_to_string(root.join("big.md")).unwrap();
    assert_eq!(
        on_disk, "L1\nL2-A\nL2-B\nL2-C\nL2-D\nL4\nL5\nL6\nL7+L8\nL9\nL10\n",
        "both splices land correctly with line-range shifting; unedited lines preserved"
    );
}

/// BL-141 Phase 3 — `open` against a `multibuffer://` relpath is
/// idempotent: returns the existing synthetic snapshot. Required
/// so the shell's `sessionManager.acquire(relpath)` path works for
/// multibuffer tabs without trying to read a non-existent file
/// from disk.
#[tokio::test]
async fn open_against_multibuffer_relpath_returns_existing_snapshot() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "doc.md", "alpha\nbeta\ngamma\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "doc.md", "line_start": 1, "line_end": 3 }
                ]
            }),
        )
        .await
        .expect("open_excerpts"),
    )
    .unwrap();
    let synthetic_relpath = snap.relpath.clone();
    let first_block_id = snap.tree.root_blocks[0];

    // Re-open against the synthetic relpath — must return the
    // same session (same root block id), not try to read
    // `multibuffer://...` from disk.
    let reopened: EditorSnapshot = serde_json::from_value(
        call(&runtime, "open", json!({ "relpath": synthetic_relpath }))
            .await
            .expect("re-open synthetic"),
    )
    .unwrap();
    assert_eq!(reopened.tree.root_blocks[0], first_block_id);
}

/// BL-141 Phase 3 — `open` against a `multibuffer://` relpath
/// that was never created surfaces a clear error rather than
/// trying to resolve it as a path.
#[tokio::test]
async fn open_against_unknown_multibuffer_relpath_errors() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let err = call(
        &runtime,
        "open",
        json!({ "relpath": "multibuffer://never-existed" }),
    )
    .await
    .expect_err("open of unknown synthetic relpath must error");
    assert!(
        err.to_string().contains("synthetic session"),
        "expected 'synthetic session' in error, got: {err}"
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

    call(&runtime, "save", json!({ "relpath": snapshot_relpath }))
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

// ─── BL-141 Approach B step 3 — refresh_excerpts ─────────────────────────────

/// Happy path — after a source file is rewritten on disk,
/// `refresh_excerpts` updates every Excerpt block's snapshot to the
/// current source slice, preserves block ids, and bumps the
/// session's revision.
#[tokio::test]
async fn refresh_excerpts_updates_snapshots_from_changed_source() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    let original = "alpha\nbeta\ngamma\ndelta\nepsilon\n";
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
    assert_eq!(snap.tree.blocks[&block_id].content, "beta\ngamma");
    let starting_revision = snap.revision;

    // Rewrite the source: lines 2-3 ("beta", "gamma") become
    // ("BETA", "GAMMA"). Surrounding lines untouched.
    let updated = "alpha\nBETA\nGAMMA\ndelta\nepsilon\n";
    write_note(&root, "doc.md", updated);

    let refreshed: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "refresh_excerpts",
            json!({ "relpath": snap.relpath }),
        )
        .await
        .expect("refresh_excerpts ok"),
    )
    .unwrap();

    // Block id stable (cursor anchors survive).
    assert_eq!(refreshed.tree.root_blocks, vec![block_id]);
    // Content refreshed.
    assert_eq!(refreshed.tree.blocks[&block_id].content, "BETA\nGAMMA");
    // Revision bumped.
    assert!(
        refreshed.revision > starting_revision,
        "refresh_excerpts must bump the synthetic session's revision"
    );
}

/// `refresh_excerpts` aggregates reads across multiple source files
/// (each unique relpath is read once).
#[tokio::test]
async fn refresh_excerpts_handles_multiple_source_files() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "a.md", "A1\nA2\nA3\nA4\nA5\n");
    write_note(&root, "b.md", "B1\nB2\nB3\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    // Non-adjacent ranges on `a.md` so `open_excerpts`'s
    // overlap-merge keeps them as distinct blocks (lines 1 + 4 with
    // a gap at lines 2-3).
    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "a.md", "line_start": 1, "line_end": 1 },
                    { "relpath": "b.md", "line_start": 2, "line_end": 3 },
                    { "relpath": "a.md", "line_start": 4, "line_end": 4 }
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();
    assert_eq!(
        snap.tree.root_blocks.len(),
        3,
        "non-adjacent same-file ranges must stay distinct"
    );

    // Both sources change externally.
    write_note(&root, "a.md", "A1!\nA2!\nA3!\nA4!\nA5!\n");
    write_note(&root, "b.md", "B1!\nB2!\nB3!\n");

    let refreshed: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "refresh_excerpts",
            json!({ "relpath": snap.relpath }),
        )
        .await
        .expect("refresh_excerpts ok"),
    )
    .unwrap();

    let contents: Vec<String> = refreshed
        .tree
        .root_blocks
        .iter()
        .map(|id| refreshed.tree.blocks[id].content.clone())
        .collect();
    // Per-excerpt slices match the new source bytes.
    assert_eq!(contents[0], "A1!");
    assert_eq!(contents[1], "B2!\nB3!");
    assert_eq!(contents[2], "A4!");
}

/// Non-synthetic sessions error — `refresh_excerpts` is a multibuffer
/// concern.
#[tokio::test]
async fn refresh_excerpts_rejects_non_synthetic_session() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "doc.md", "hello\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    // Open the source file normally (non-synthetic session).
    call(&runtime, "open", json!({ "relpath": "doc.md" }))
        .await
        .expect("open ok");

    let err = call(&runtime, "refresh_excerpts", json!({ "relpath": "doc.md" }))
        .await
        .expect_err("refresh_excerpts must reject a non-synthetic session");
    assert!(
        err.to_string().contains("multibuffer"),
        "expected multibuffer error, got: {err}"
    );
}

/// BL-141 Approach B step 4A — after a save splices a grown
/// excerpt into the source, the synthetic session's stored
/// `(line_start, line_end)` updates to reflect the new line range.
/// A subsequent `refresh_excerpts` is then a no-op (slice at the
/// new range == current content), so the multibuffer doesn't show
/// stale lines.
#[tokio::test]
async fn save_reflows_excerpt_ranges_to_match_post_splice_positions() {
    use nexus_editor::{Operation, Transaction, TransactionMetadata};

    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    write_note(&root, "doc.md", "L1\nL2\nL3\nL4\nL5\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    // Two excerpts in the same file: lines 2..=2 and lines 4..=4.
    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "doc.md", "line_start": 2, "line_end": 2 },
                    { "relpath": "doc.md", "line_start": 4, "line_end": 4 }
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();
    let first = snap.tree.root_blocks[0];
    let second = snap.tree.root_blocks[1];
    assert_eq!(snap.tree.blocks[&first].content, "L2");
    assert_eq!(snap.tree.blocks[&second].content, "L4");

    // Grow the first excerpt: "L2" → "L2\nL2.5" (1 → 2 lines).
    let tx = Transaction::new(
        vec![Operation::UpdateBlockContent {
            id: first,
            old_content: "L2".into(),
            new_content: "L2\nL2.5".into(),
            old_annotations: Vec::new(),
            new_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    call(
        &runtime,
        "apply_transaction",
        json!({ "relpath": snap.relpath, "transaction": tx }),
    )
    .await
    .expect("apply_transaction ok");

    // Save splices both excerpts back into the source. After save:
    // source should be "L1\nL2\nL2.5\nL3\nL4\nL5\n" (6 lines) and
    // the second excerpt's stored line should shift from 4 → 5.
    call(&runtime, "save", json!({ "relpath": snap.relpath }))
        .await
        .expect("save ok");

    let on_disk = fs::read_to_string(root.join("doc.md")).unwrap();
    assert_eq!(on_disk, "L1\nL2\nL2.5\nL3\nL4\nL5\n");

    // get_tree after save: the second excerpt's stored line should
    // now point at the post-splice position (line 5 in the new
    // source, which still contains "L4").
    let after: EditorSnapshot = serde_json::from_value(
        call(&runtime, "get_tree", json!({ "relpath": snap.relpath }))
            .await
            .expect("get_tree ok"),
    )
    .unwrap();
    let second_ty = serde_json::to_value(&after.tree.blocks[&second].ty).unwrap();
    assert_eq!(second_ty["line_start"], 5);
    assert_eq!(second_ty["line_end"], 5);
    // The first excerpt's range grew from 1 line to 2.
    let first_ty = serde_json::to_value(&after.tree.blocks[&first].ty).unwrap();
    assert_eq!(first_ty["line_start"], 2);
    assert_eq!(first_ty["line_end"], 3);

    // Refresh is now a no-op for the second excerpt's content (the
    // slice at the updated range still matches the snapshot).
    let refreshed: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "refresh_excerpts",
            json!({ "relpath": snap.relpath }),
        )
        .await
        .expect("refresh_excerpts ok"),
    )
    .unwrap();
    assert_eq!(refreshed.tree.blocks[&second].content, "L4");
    // The first excerpt is also unchanged: its new range (2..=3)
    // reads "L2\nL2.5" from the new source — same as the snapshot.
    assert_eq!(refreshed.tree.blocks[&first].content, "L2\nL2.5");
}

/// BL-141 Approach B step 4B — when another tab prepends lines to a
/// source file, `refresh_excerpts` finds the original snapshot text
/// as a unique contiguous line sequence in the new source and
/// updates the Excerpt's stored `(line_start, line_end)`. The
/// snapshot content is preserved (the user is still looking at the
/// same lines, just at a new line number).
#[tokio::test]
async fn refresh_excerpts_relocates_anchors_on_external_prepend() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    // Unique 2-line excerpt content so the content-search finds
    // exactly one match in both pre- and post-prepend sources.
    let original = "header\nORIGINAL LINE A\nORIGINAL LINE B\nfooter\n";
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
    assert_eq!(
        snap.tree.blocks[&block_id].content,
        "ORIGINAL LINE A\nORIGINAL LINE B"
    );
    // Verify starting anchors.
    let pre_ty = serde_json::to_value(&snap.tree.blocks[&block_id].ty).unwrap();
    assert_eq!(pre_ty["line_start"], 2);
    assert_eq!(pre_ty["line_end"], 3);

    // Another tab prepends 2 new header lines. The original snapshot
    // text now lives at lines 4..=5 instead of 2..=3.
    let prepended =
        "NEW header 1\nNEW header 2\nheader\nORIGINAL LINE A\nORIGINAL LINE B\nfooter\n";
    write_note(&root, "doc.md", prepended);

    let refreshed: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "refresh_excerpts",
            json!({ "relpath": snap.relpath }),
        )
        .await
        .expect("refresh_excerpts ok"),
    )
    .unwrap();

    // Content preserved — the user is still looking at the same lines.
    assert_eq!(
        refreshed.tree.blocks[&block_id].content,
        "ORIGINAL LINE A\nORIGINAL LINE B"
    );
    // Anchors updated to the new line numbers.
    let post_ty = serde_json::to_value(&refreshed.tree.blocks[&block_id].ty).unwrap();
    assert_eq!(post_ty["line_start"], 4);
    assert_eq!(post_ty["line_end"], 5);
}

/// Step 4B — when the relocation is ambiguous (same content appears
/// in multiple places in the new source), the refresh falls back to
/// the baseline slice-and-overwrite behaviour rather than picking a
/// match arbitrarily.
#[tokio::test]
async fn refresh_excerpts_falls_back_to_slice_on_ambiguous_relocation() {
    let forge = scratch_forge();
    let root = forge.path().to_path_buf();
    // Excerpt content "REPEAT" — a 1-line snapshot that's trivially
    // ambiguous if the source has multiple "REPEAT" lines.
    write_note(&root, "doc.md", "header\nREPEAT\nfooter\n");

    let runtime = build_cli_runtime(root.clone()).expect("build runtime");

    let snap: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "open_excerpts",
            json!({
                "items": [
                    { "relpath": "doc.md", "line_start": 2, "line_end": 2 }
                ]
            }),
        )
        .await
        .expect("open_excerpts ok"),
    )
    .unwrap();
    let block_id = snap.tree.root_blocks[0];

    // Rewrite the source: line 2 becomes "EDITED" but "REPEAT"
    // appears twice elsewhere. The original "REPEAT" snapshot is now
    // ambiguous, so the relocation refuses to guess and falls back
    // to overwriting the content with the new line 2.
    let edited = "header\nEDITED\nREPEAT\nREPEAT\nfooter\n";
    write_note(&root, "doc.md", edited);

    let refreshed: EditorSnapshot = serde_json::from_value(
        call(
            &runtime,
            "refresh_excerpts",
            json!({ "relpath": snap.relpath }),
        )
        .await
        .expect("refresh_excerpts ok"),
    )
    .unwrap();

    // Fallback: snapshot now reflects the line-2 content in the new
    // source (anchors stay at 2..=2 → "EDITED").
    assert_eq!(refreshed.tree.blocks[&block_id].content, "EDITED");
    let post_ty = serde_json::to_value(&refreshed.tree.blocks[&block_id].ty).unwrap();
    assert_eq!(post_ty["line_start"], 2);
    assert_eq!(post_ty["line_end"], 2);
}

/// Unknown relpath surfaces as a session-not-found error.
#[tokio::test]
async fn refresh_excerpts_rejects_unknown_session() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");
    let err = call(
        &runtime,
        "refresh_excerpts",
        json!({ "relpath": "multibuffer://does-not-exist" }),
    )
    .await
    .expect_err("refresh_excerpts must reject an unknown relpath");
    let msg = err.to_string();
    assert!(
        msg.contains("not found") || msg.contains("acquire") || msg.contains("session"),
        "expected session-missing error, got: {msg}"
    );
}
