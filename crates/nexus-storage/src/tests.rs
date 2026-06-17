//! R8 / #191 — internal test module lifted out of `lib.rs` (which exceeded
//! 3,000 LoC). Kept as an in-crate `#[cfg(test)] mod tests;` (vs. an external
//! `tests/` integration file) because the tests reach for the crate-private
//! helpers (`resolve_within`, `infer_file_type`, `coerce_property_value`, …)
//! through `super::*`. An external integration test would only see the
//! public API surface.

use super::*;
use tempfile::TempDir;

fn tmp() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

// ── 1. init_creates_working_engine ────────────────────────────────────────

#[test]
fn init_creates_working_engine() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    assert!(
        engine.forge().forge_dir().join("index.db").exists(),
        ".forge/index.db should exist"
    );
    assert!(engine.forge().notes_dir().exists(), "notes/ should exist");
}

// ── 2. write_and_read_file ────────────────────────────────────────────────

#[test]
fn write_and_read_file() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    let content = b"# Hello\n\nWorld paragraph.";
    engine.write_file("notes/hello.md", content).expect("write");

    let read_back = engine.read_file("notes/hello.md").expect("read");
    assert_eq!(read_back, content);
}

// ── 3. write_file_is_indexed ──────────────────────────────────────────────

#[test]
fn write_file_is_indexed() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    engine
        .write_file("notes/indexed.md", b"# Indexed\n\nContent.")
        .expect("write");

    assert!(engine.file_exists("notes/indexed.md").expect("file_exists"));
    let files = engine.list_files("notes/").expect("list_files");
    assert_eq!(files.len(), 1);
}

// ── 4. delete_file_removes_from_index ────────────────────────────────────

#[test]
fn delete_file_removes_from_index() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    engine
        .write_file("notes/gone.md", b"# Gone\n\nBye.")
        .expect("write");
    assert!(engine.file_exists("notes/gone.md").expect("file_exists"));

    engine.delete_file("notes/gone.md").expect("delete");

    assert!(!engine.file_exists("notes/gone.md").expect("file_exists"));
    assert!(
        !dir.path().join("notes/gone.md").exists(),
        "file should be removed from disk"
    );
}

// ── 4b. Graph + SQLite stay consistent across write/delete cycles ─────────
//
// Contract test for the post-commit invariant: every write_file /
// delete_file path must update the in-memory knowledge graph only AFTER
// the SQLite transaction commits. A regression that removed the graph
// mutation block, or that left the graph mutated when the DB write
// failed, would surface here as a divergence between `backlinks()`
// (graph-backed) and `query_files()` (DB-backed).
#[test]
fn graph_and_index_consistent_after_write_then_delete() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    engine
        .write_file("notes/target.md", b"# Target\n")
        .expect("write target");
    engine
        .write_file(
            "notes/source.md",
            b"# Source\n\nLinks to [[notes/target.md]].",
        )
        .expect("write source");

    // After write_file commits, the graph must reflect the link.
    let backs = engine.backlinks("notes/target.md").expect("backlinks");
    assert!(
        backs.iter().any(|b| b.source_path == "notes/source.md"),
        "graph missing backlink from source after commit; got {backs:?}",
    );
    assert_eq!(
        engine
            .query_files(&FileFilter::default())
            .expect("query")
            .len(),
        2,
        "DB should hold both files post-commit",
    );

    // Delete the source. Both the DB row and the graph node must go away
    // together — if the graph still remembers the source as a linker,
    // backlinks(target) will still surface it.
    engine.delete_file("notes/source.md").expect("delete");

    let backs_after = engine.backlinks("notes/target.md").expect("backlinks");
    assert!(
        backs_after.is_empty(),
        "graph still reports backlinks after source deletion; got {backs_after:?}",
    );
    assert_eq!(
        engine
            .query_files(&FileFilter::default())
            .expect("query")
            .len(),
        1,
        "only the target should remain in the DB",
    );
}

// ── 5. query_blocks_after_write ───────────────────────────────────────────

#[test]
fn query_blocks_after_write() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    engine
        .write_file("notes/blocks.md", b"# Title\n\nParagraph text.")
        .expect("write");

    let files = engine.list_files("notes/").expect("list_files");
    assert_eq!(files.len(), 1);

    // Get the file record to obtain the file ID.
    let filter = FileFilter::default();
    let records = engine.query_files(&filter).expect("query_files");
    assert_eq!(records.len(), 1);

    let blocks = engine.query_blocks(records[0].id).expect("query_blocks");
    assert!(
        blocks.len() >= 2,
        "expected >= 2 blocks, got {}",
        blocks.len()
    );
}

// ── 6. query_tags_after_write ─────────────────────────────────────────────

#[test]
fn query_tags_after_write() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    engine
        .write_file("notes/tagged.md", b"# Tagged\n\nThis has #rust tag.")
        .expect("write");

    let tags = engine.query_tags("rust").expect("query_tags");
    assert_eq!(
        tags.len(),
        1,
        "expected 1 tag result for 'rust', got {}",
        tags.len()
    );
}

// ── BL-114: code-symbol index — write_file path ───────────────────────────

#[test]
fn write_file_indexes_code_symbols() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine
        .write_file("notes/lib.rs", b"pub fn hello() {}\npub struct Counter;\n")
        .expect("write");
    let rows = engine
        .query_symbols(&code_index::SymbolFilter {
            path: Some("notes/lib.rs".into()),
            ..Default::default()
        })
        .expect("query");
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"hello"));
    assert!(names.contains(&"Counter"));
}

#[test]
fn write_file_skips_non_code_files() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine
        .write_file("notes/note.md", b"# Title\n\nBody paragraph.")
        .expect("write");
    let rows = engine
        .query_symbols(&code_index::SymbolFilter::default())
        .expect("query");
    assert!(rows.is_empty(), "markdown should not produce code symbols");
}

#[test]
fn write_file_replaces_prior_symbols() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine
        .write_file("notes/a.rs", b"pub fn old() {}\n")
        .expect("write v1");
    engine
        .write_file("notes/a.rs", b"pub fn fresh() {}\n")
        .expect("write v2");
    let rows = engine
        .query_symbols(&code_index::SymbolFilter::default())
        .expect("query");
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(!names.contains(&"old"), "stale row from v1 must be gone");
    assert!(names.contains(&"fresh"));
}

#[test]
fn delete_file_drops_code_symbols() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine
        .write_file("notes/gone.rs", b"pub fn x() {}\n")
        .expect("write");
    engine.delete_file("notes/gone.rs").expect("delete");
    let rows = engine
        .query_symbols(&code_index::SymbolFilter::default())
        .expect("query");
    assert!(rows.is_empty());
}

#[test]
fn rebuild_index_indexes_code_symbols_from_disk() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    std::fs::write(
        dir.path().join("notes/script.py"),
        "def greet(name):\n    return name\n",
    )
    .expect("write py");
    std::fs::write(
        dir.path().join("notes/main.go"),
        "package main\nfunc Greet() string { return \"hi\" }\n",
    )
    .expect("write go");
    engine.rebuild_index().expect("rebuild");
    let py_rows = engine
        .query_symbols(&code_index::SymbolFilter {
            name: Some("greet".into()),
            ..Default::default()
        })
        .expect("query");
    assert_eq!(py_rows.len(), 1);
    assert_eq!(py_rows[0].language, "python");
    let go_rows = engine
        .query_symbols(&code_index::SymbolFilter {
            name: Some("Greet".into()),
            ..Default::default()
        })
        .expect("query");
    assert_eq!(go_rows.len(), 1);
    assert_eq!(go_rows[0].language, "go");
}

#[test]
fn reconcile_rename_repaths_code_symbols() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine
        .write_file("notes/old.rs", b"pub fn rename_me() {}\n")
        .expect("write");
    // Rename on disk + reconcile.
    std::fs::rename(
        dir.path().join("notes/old.rs"),
        dir.path().join("notes/new.rs"),
    )
    .expect("rename");
    engine.reconcile_index().expect("reconcile");
    let rows = engine
        .query_symbols(&code_index::SymbolFilter {
            name: Some("rename_me".into()),
            ..Default::default()
        })
        .expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "notes/new.rs");
}

// ── 7. rebuild_index_reindexes_all ────────────────────────────────────────

#[test]
fn rebuild_index_reindexes_all() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    engine
        .write_file("notes/a.md", b"# Alpha\n\nContent A.")
        .expect("write a");
    engine
        .write_file("notes/b.md", b"# Beta\n\nContent B.")
        .expect("write b");

    let stats = engine.rebuild_index().expect("rebuild_index");
    assert_eq!(
        stats.files_processed, 2,
        "expected 2 files_processed, got {}",
        stats.files_processed
    );
}

// ── 7b. rebuild_index also refreshes the FTS search index ────────────────
//
// Contract test: callers of rebuild_index do NOT need to invoke
// rebuild_search_index afterwards. If somebody decouples the two paths,
// search will keep returning blocks indexed against the pre-rebuild row
// ids, which manifests here as a missing search hit for content the
// SQL rebuild definitely picked up.
#[test]
fn rebuild_index_also_refreshes_search() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    // Seed two files and warm the FTS index for the initial content.
    engine
        .write_file("notes/old.md", b"# Old\n\nstale-only-token\n")
        .expect("write old");
    engine.rebuild_search_index().expect("seed search");
    let hits = engine.search("stale-only-token", 10).expect("search");
    assert!(!hits.is_empty(), "search must find seeded content");

    // Drop the old file from disk and add a brand-new one whose
    // distinguishing token only exists post-rebuild.
    std::fs::remove_file(dir.path().join("notes/old.md")).expect("rm old");
    std::fs::write(
        dir.path().join("notes/fresh.md"),
        b"# Fresh\n\nzephyr-token-92a1\n",
    )
    .expect("write fresh");

    // rebuild_index alone must (a) reflect the disk state in SQL and
    // (b) refresh the FTS so search finds the new token AND no longer
    // surfaces the deleted one.
    engine.rebuild_index().expect("rebuild_index");

    let fresh_hits = engine.search("zephyr-token-92a1", 10).expect("search");
    assert!(
        !fresh_hits.is_empty(),
        "search must find post-rebuild content without an explicit rebuild_search_index call",
    );
    let stale_hits = engine.search("stale-only-token", 10).expect("search");
    assert!(
        stale_hits.is_empty(),
        "search must not return content for files removed before rebuild_index; got {stale_hits:?}",
    );
}

// ── 8. read_nonexistent_file_returns_error ────────────────────────────────

#[test]
fn read_nonexistent_file_returns_error() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    let result = engine.read_file("notes/nonexistent.md");
    assert!(
        matches!(result, Err(StorageError::FileNotFound(_))),
        "expected FileNotFound, got: {result:?}"
    );
}

// ── 9. write_raw_bypasses_index ───────────────────────────────────────────

#[test]
fn write_raw_bypasses_index() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    // Write a markdown-like file via write_raw to a path the index would
    // normally pick up. Contains a tag that would show up in query_tags
    // if the indexing pipeline ran.
    let rel = ".forge/workspace.json";
    let content = b"# Raw\n\nHas a #rawtag inside.";
    engine.write_raw(rel, content).expect("write_raw");

    // Bytes are on disk, exactly as written.
    let abs = dir.path().join(rel);
    assert!(abs.exists(), "file must exist on disk after write_raw");
    assert_eq!(
        std::fs::read(&abs).expect("read back"),
        content,
        "disk content must match bytes passed to write_raw"
    );

    // Index must NOT have picked up the file: no row in the files table,
    // no tag inserted, no graph node created. Contrast with write_file
    // which always runs the full pipeline (see write_and_read_file test).
    assert!(
        !engine.file_exists(rel).expect("file_exists"),
        "write_raw must not insert an index row"
    );
    let tags = engine.query_tags("rawtag").expect("query_tags");
    assert!(
        tags.is_empty(),
        "write_raw must not index tags, got {tags:?}"
    );
    let stats = engine.graph_stats().expect("graph_stats");
    assert_eq!(
        stats.node_count, 0,
        "write_raw must not add graph nodes, got {} nodes",
        stats.node_count
    );
}

// ── 10. canvas_write_read_patch_roundtrip ─────────────────────────────────

#[test]
fn canvas_write_read_patch_roundtrip() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    let mut initial = CanvasFile::default();
    initial.nodes.push(CanvasNode {
        id: "a".to_string(),
        node_type: CanvasNodeType::Text,
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
        color: None,
        label: None,
        collapsed: false,
        file: None,
        text: Some("hi".to_string()),
        url: None,
        source: None,
        command: None,
        extra: serde_json::Map::new(),
    });

    engine
        .write_canvas("boards/one.canvas", &initial)
        .expect("write_canvas");

    let nodes = engine
        .canvas_nodes_by_path("boards/one.canvas")
        .expect("nodes");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].node_id, "a");

    engine
        .patch_canvas(
            "boards/one.canvas",
            &[CanvasPatchOp::NodeMove {
                id: "a".to_string(),
                x: 42.0,
                y: 7.0,
            }],
        )
        .expect("patch_canvas");

    let parsed = engine
        .read_canvas("boards/one.canvas")
        .expect("read_canvas");
    assert!((parsed.nodes[0].x - 42.0).abs() < f64::EPSILON);
    assert!((parsed.nodes[0].y - 7.0).abs() < f64::EPSILON);

    let after = engine
        .canvas_nodes_by_path("boards/one.canvas")
        .expect("nodes2");
    assert_eq!(after.len(), 1);
    assert!((after[0].x - 42.0).abs() < f64::EPSILON);
}

// ── 11. canvas_queries_by_path_on_missing_return_empty ────────────────────

#[test]
fn canvas_queries_by_path_on_missing_return_empty() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    assert!(engine
        .canvas_nodes_by_path("nope.canvas")
        .expect("nodes")
        .is_empty());
    assert!(engine
        .canvas_edges_by_path("nope.canvas")
        .expect("edges")
        .is_empty());
}

// ── 12. base_record_crud_roundtrip ────────────────────────────────────────

#[test]
fn base_record_crud_roundtrip() {
    use nexus_types::bases::{Base, BaseMetadata, BaseRecord, BaseSchema};

    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    let base_rel = "tasks.bases";
    let abs = dir.path().join(base_rel);
    let mut fields = serde_json::Map::new();
    fields.insert(
        "title".to_string(),
        serde_json::json!({ "type": "title", "required": true }),
    );
    let seed = Base {
        name: "Tasks".to_string(),
        schema: BaseSchema {
            version: "1.0".to_string(),
            fields,
        },
        records: Vec::new(),
        views: Vec::new(),
        relations: Vec::new(),
        metadata: BaseMetadata::default(),
    };
    nexus_types::bases::save_base(&abs, &seed).expect("save seed");
    engine.index_base(base_rel, &seed).expect("index seed");

    // Create with server-generated id.
    let created = engine
        .base_record_create(
            base_rel,
            BaseRecord {
                id: String::new(),
                deleted_at: None,
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("title".to_string(), serde_json::json!("Buy milk"));
                    m
                },
            },
        )
        .expect("create");
    assert!(!created.id.is_empty(), "id should be generated");
    let created_id = created.id.clone();

    // Update — patch one field.
    let patch = {
        let mut m = serde_json::Map::new();
        m.insert("title".to_string(), serde_json::json!("Buy oat milk"));
        m
    };
    let updated = engine
        .base_record_update(base_rel, &created_id, &patch)
        .expect("update");
    assert_eq!(updated.fields.get("title").unwrap(), "Buy oat milk");

    // Re-read from disk to confirm round-trip.
    let reloaded = nexus_types::bases::load_base(&abs).expect("load");
    assert_eq!(reloaded.records.len(), 1);
    assert_eq!(
        reloaded.records[0].fields.get("title").unwrap(),
        "Buy oat milk"
    );

    // Delete.
    engine
        .base_record_delete(base_rel, &created_id)
        .expect("delete");
    let reloaded = nexus_types::bases::load_base(&abs).expect("load2");
    assert!(reloaded.records.is_empty());

    // Delete again — idempotent no-op.
    engine
        .base_record_delete(base_rel, &created_id)
        .expect("delete noop");
}

// ── 13. base_record_create_rejects_duplicate_id ───────────────────────────

#[test]
fn base_record_create_rejects_duplicate_id() {
    use nexus_types::bases::{Base, BaseMetadata, BaseRecord, BaseSchema};

    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    let base_rel = "d.bases";
    let seed = Base {
        name: "D".to_string(),
        schema: BaseSchema {
            version: "1.0".to_string(),
            fields: serde_json::Map::new(),
        },
        records: vec![BaseRecord {
            id: "r1".into(),
            deleted_at: None,
            fields: serde_json::Map::new(),
        }],
        views: Vec::new(),
        relations: Vec::new(),
        metadata: BaseMetadata::default(),
    };
    nexus_types::bases::save_base(&dir.path().join(base_rel), &seed).expect("save");
    engine.index_base(base_rel, &seed).expect("index");

    let err = engine
        .base_record_create(
            base_rel,
            BaseRecord {
                id: "r1".into(),
                deleted_at: None,
                fields: serde_json::Map::new(),
            },
        )
        .expect_err("duplicate should fail");
    assert!(matches!(err, StorageError::CorruptFile { .. }));
}

// ── 14. base_record_update_unknown_id_errors ──────────────────────────────

#[test]
fn base_record_update_unknown_id_errors() {
    use nexus_types::bases::{Base, BaseMetadata, BaseSchema};

    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    let base_rel = "u.bases";
    let seed = Base {
        name: "U".to_string(),
        schema: BaseSchema {
            version: "1.0".to_string(),
            fields: serde_json::Map::new(),
        },
        records: Vec::new(),
        views: Vec::new(),
        relations: Vec::new(),
        metadata: BaseMetadata::default(),
    };
    nexus_types::bases::save_base(&dir.path().join(base_rel), &seed).expect("save");
    engine.index_base(base_rel, &seed).expect("index");

    let err = engine
        .base_record_update(base_rel, "ghost", &serde_json::Map::new())
        .expect_err("unknown id should fail");
    assert!(matches!(err, StorageError::FileNotFound(_)));
}

// ── 15. base_property_crud ────────────────────────────────────────────────

#[test]
fn base_property_crud() {
    use nexus_types::bases::{Base, BaseMetadata, BaseRecord, BaseSchema};

    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    let base_rel = "p.bases";
    let abs = dir.path().join(base_rel);
    let seed = Base {
        name: "P".to_string(),
        schema: BaseSchema {
            version: "1.0".to_string(),
            fields: {
                let mut m = serde_json::Map::new();
                m.insert("legacy".to_string(), serde_json::json!({ "type": "text" }));
                m
            },
        },
        records: vec![BaseRecord {
            id: "r1".into(),
            deleted_at: None,
            fields: {
                let mut m = serde_json::Map::new();
                m.insert("legacy".to_string(), serde_json::json!("stale"));
                m
            },
        }],
        views: Vec::new(),
        relations: Vec::new(),
        metadata: BaseMetadata::default(),
    };
    nexus_types::bases::save_base(&abs, &seed).expect("save");
    engine.index_base(base_rel, &seed).expect("index");

    // Create.
    engine
        .base_property_create(base_rel, "title", serde_json::json!({ "type": "title" }))
        .expect("create");
    let loaded = nexus_types::bases::load_base(&abs).expect("load");
    assert!(loaded.schema.fields.contains_key("title"));

    // Duplicate create → error.
    let err = engine
        .base_property_create(base_rel, "title", serde_json::json!({ "type": "text" }))
        .expect_err("dup");
    assert!(matches!(err, StorageError::CorruptFile { .. }));

    // Update.
    engine
        .base_property_update(
            base_rel,
            "title",
            &serde_json::json!({ "type": "text", "required": true }),
            false,
        )
        .expect("update");
    let loaded = nexus_types::bases::load_base(&abs).expect("load2");
    assert_eq!(
        loaded.schema.fields["title"].get("required"),
        Some(&serde_json::Value::Bool(true))
    );

    // Update unknown → error.
    let err = engine
        .base_property_update(base_rel, "nope", &serde_json::json!({}), false)
        .expect_err("unknown");
    assert!(matches!(err, StorageError::FileNotFound(_)));

    // Delete drops record key.
    engine
        .base_property_delete(base_rel, "legacy")
        .expect("delete legacy");
    let loaded = nexus_types::bases::load_base(&abs).expect("load3");
    assert!(!loaded.records[0].fields.contains_key("legacy"));

    // Delete unknown → no-op.
    engine
        .base_property_delete(base_rel, "ghost")
        .expect("delete ghost");
}

// ── 15a. base_record_soft_delete + restore ────────────────────────────────

#[test]
fn base_record_soft_delete_and_restore() {
    use nexus_types::bases::BaseSchema;

    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    let base_rel = "s.bases";

    let schema = BaseSchema {
        version: "1.0".to_string(),
        fields: {
            let mut m = serde_json::Map::new();
            m.insert(
                "title".to_string(),
                serde_json::json!({ "type": "title", "required": true, "primary": true }),
            );
            m
        },
    };
    engine
        .base_create(base_rel, &schema, Vec::new())
        .expect("create");

    let record = nexus_types::bases::BaseRecord {
        id: String::new(),
        deleted_at: None,
        fields: {
            let mut m = serde_json::Map::new();
            m.insert("title".to_string(), serde_json::json!("Hello"));
            m
        },
    };
    let stored = engine.base_record_create(base_rel, record).expect("record");

    engine
        .base_record_soft_delete(base_rel, &stored.id)
        .expect("soft delete");
    let abs = dir.path().join(base_rel);
    let loaded = nexus_types::bases::load_base(&abs).expect("load1");
    assert_eq!(
        loaded.records.len(),
        1,
        "soft-delete keeps the record on disk"
    );
    assert!(
        loaded.records[0].deleted_at.is_some(),
        "deleted_at should be set after soft delete",
    );

    // Restoring clears the slot.
    engine
        .base_record_restore(base_rel, &stored.id)
        .expect("restore");
    let loaded = nexus_types::bases::load_base(&abs).expect("load2");
    assert!(
        loaded.records[0].deleted_at.is_none(),
        "deleted_at should be cleared after restore",
    );

    // Unknown id → no-op.
    engine
        .base_record_soft_delete(base_rel, "ghost")
        .expect("ghost soft delete is no-op");
    engine
        .base_record_restore(base_rel, "ghost")
        .expect("ghost restore is no-op");
}

// ── 15b. base_create + property rename + retype-migration ─────────────────

#[test]
fn base_create_and_property_rename_retype() {
    use nexus_types::bases::BaseSchema;

    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    let base_rel = "new.bases";

    let schema = BaseSchema {
        version: "1.0".to_string(),
        fields: {
            let mut m = serde_json::Map::new();
            m.insert(
                "title".to_string(),
                serde_json::json!({ "type": "title", "required": true, "primary": true }),
            );
            m.insert("count".to_string(), serde_json::json!({ "type": "number" }));
            m
        },
    };

    // base_create — empty.
    let created = engine
        .base_create(base_rel, &schema, Vec::new())
        .expect("create");
    assert_eq!(created.name, "new");
    assert_eq!(created.records.len(), 0);

    // Seed a record with a numeric value, then retype count → text
    // with migration and verify it serialized as a string.
    let record = nexus_types::bases::BaseRecord {
        id: String::new(),
        deleted_at: None,
        fields: {
            let mut m = serde_json::Map::new();
            m.insert("title".to_string(), serde_json::json!("Hello"));
            m.insert("count".to_string(), serde_json::json!(42));
            m
        },
    };
    let stored = engine.base_record_create(base_rel, record).expect("record");
    assert!(!stored.id.is_empty());

    engine
        .base_property_update(
            base_rel,
            "count",
            &serde_json::json!({ "type": "text" }),
            true,
        )
        .expect("retype with migrate");
    let abs = dir.path().join(base_rel);
    let loaded = nexus_types::bases::load_base(&abs).expect("load1");
    assert_eq!(loaded.records[0].fields["count"], serde_json::json!("42"));

    // Rename column → schema key moves and record field key moves.
    engine
        .base_property_rename(base_rel, "count", "total")
        .expect("rename");
    let loaded = nexus_types::bases::load_base(&abs).expect("load2");
    assert!(loaded.schema.fields.contains_key("total"));
    assert!(!loaded.schema.fields.contains_key("count"));
    assert_eq!(loaded.records[0].fields["total"], serde_json::json!("42"));
    assert!(!loaded.records[0].fields.contains_key("count"));

    // Rename collision → error.
    let err = engine
        .base_property_rename(base_rel, "total", "title")
        .expect_err("collision");
    assert!(matches!(err, StorageError::CorruptFile { .. }));

    // base_create on existing path → error.
    let err = engine
        .base_create(base_rel, &schema, Vec::new())
        .expect_err("exists");
    assert!(matches!(err, StorageError::CorruptFile { .. }));
}

// ── 16. base_view_crud ────────────────────────────────────────────────────

#[test]
fn base_view_crud() {
    use nexus_types::bases::{Base, BaseMetadata, BaseSchema, BaseView, ViewType};

    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    let base_rel = "v.bases";
    let abs = dir.path().join(base_rel);
    let seed = Base {
        name: "V".to_string(),
        schema: BaseSchema {
            version: "1.0".to_string(),
            fields: serde_json::Map::new(),
        },
        records: Vec::new(),
        views: Vec::new(),
        relations: Vec::new(),
        metadata: BaseMetadata::default(),
    };
    nexus_types::bases::save_base(&abs, &seed).expect("save");
    engine.index_base(base_rel, &seed).expect("index");

    let board = BaseView {
        name: "Board".to_string(),
        view_type: ViewType::Kanban,
        fields: vec!["title".to_string()],
        sort: Vec::new(),
        filter: Vec::new(),
        group_field: Some("status".to_string()),
        date_field: None,
        end_field: None,
    };
    engine
        .base_view_create(base_rel, board.clone())
        .expect("create");
    let loaded = nexus_types::bases::load_base(&abs).expect("load");
    assert_eq!(loaded.views.len(), 1);
    assert_eq!(loaded.views[0].name, "Board");

    let err = engine
        .base_view_create(base_rel, board.clone())
        .expect_err("dup");
    assert!(matches!(err, StorageError::CorruptFile { .. }));

    let mut updated = board.clone();
    updated.group_field = Some("priority".to_string());
    engine.base_view_update(base_rel, updated).expect("update");
    let loaded = nexus_types::bases::load_base(&abs).expect("load2");
    assert_eq!(loaded.views[0].group_field.as_deref(), Some("priority"));

    let ghost = BaseView {
        name: "Ghost".to_string(),
        ..board
    };
    let err = engine
        .base_view_update(base_rel, ghost)
        .expect_err("unknown");
    assert!(matches!(err, StorageError::FileNotFound(_)));

    engine.base_view_delete(base_rel, "Board").expect("delete");
    let loaded = nexus_types::bases::load_base(&abs).expect("load3");
    assert!(loaded.views.is_empty());

    engine
        .base_view_delete(base_rel, "noop")
        .expect("noop delete");
}

// ── 17. open_nonexistent_forge_returns_error ──────────────────────────────

#[test]
fn open_nonexistent_forge_returns_error() {
    let dir = tmp();
    let result = StorageEngine::open(dir.path(), &StorageConfig::default());
    assert!(
        matches!(result, Err(StorageError::FileNotFound(_))),
        "expected FileNotFound, got: {result:?}"
    );
}

// ── Phase 5.1 (RFC 0005): com.nexus.storage::edit (hashline) ───────────────

/// Build the `edit` args JSON for a single-section patch against the live
/// content of `path` (TAG computed from what the engine currently stores).
fn edit_args(engine: &StorageEngine, path: &str, ops: &str) -> serde_json::Value {
    let stored = engine.read_file(path).expect("read for tag");
    let text = String::from_utf8(stored).expect("utf8");
    let tag = nexus_hashline::tag(&text);
    serde_json::json!({ "patch": format!("[{path}#{tag}]\n{ops}") })
}

#[test]
fn edit_applies_hashline_patch_and_reindexes() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine
        .write_file("notes/edit.md", b"alpha\nbeta\ngamma\n")
        .expect("write");

    let args = edit_args(&engine, "notes/edit.md", "SWAP 2.=2:\n+BETA\n");
    let snaps = nexus_hashline::SnapshotStore::new();
    let reply = crate::handlers::files::edit_file(&engine, &snaps, &args).expect("edit ok");

    // Result reports one applied file, no conflicts.
    assert_eq!(reply["files"].as_array().unwrap().len(), 1);
    assert_eq!(reply["files"][0]["path"], "notes/edit.md");
    assert_eq!(reply["files"][0]["status"], "applied");
    assert_eq!(reply["conflicts"].as_array().unwrap().len(), 0);

    // The file on disk reflects the patch.
    let after = engine.read_file("notes/edit.md").expect("read back");
    assert_eq!(after, b"alpha\nBETA\ngamma\n");
}

#[test]
fn edit_is_atomic_across_sections() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine.write_file("notes/a.md", b"one\n").expect("write a");
    engine.write_file("notes/b.md", b"two\n").expect("write b");

    // Section for a.md is valid; section for b.md carries a stale TAG.
    let tag_a = nexus_hashline::tag("one\n");
    let patch = format!("[notes/a.md#{tag_a}]\nSWAP 1.=1:\n+ONE\n\n[notes/b.md#0000]\nSWAP 1.=1:\n+TWO\n");
    let err = crate::handlers::files::edit_file(
        &engine,
        &nexus_hashline::SnapshotStore::new(),
        &serde_json::json!({ "patch": patch }),
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("notes/b.md"), "error names the stale file");

    // Neither file was written (all-or-nothing): a.md is untouched.
    assert_eq!(engine.read_file("notes/a.md").unwrap(), b"one\n");
}

#[test]
fn edit_stale_tag_errors_without_writing() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine.write_file("notes/s.md", b"current\n").expect("write");

    let patch = "[notes/s.md#0000]\nSWAP 1.=1:\n+x\n";
    let err = crate::handlers::files::edit_file(
        &engine,
        &nexus_hashline::SnapshotStore::new(),
        &serde_json::json!({ "patch": patch }),
    )
    .unwrap_err();
    assert!(format!("{err:?}").to_lowercase().contains("tag"), "stale-tag error: {err:?}");
    assert_eq!(engine.read_file("notes/s.md").unwrap(), b"current\n");
}

#[test]
fn edit_rejects_malformed_patch() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    let err = crate::handlers::files::edit_file(
        &engine,
        &nexus_hashline::SnapshotStore::new(),
        &serde_json::json!({ "patch": "not a patch" }),
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("malformed"), "got: {err:?}");
}

// ── Phase 5.1 PR B2: read-snapshot store + 3-way-merge recovery ────────────

#[test]
fn read_file_handler_records_snapshot_for_later_merge() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine.write_file("notes/r.md", b"hello\n").expect("write");

    let mut snaps = nexus_hashline::SnapshotStore::new();
    let reply = crate::handlers::files::read_file(
        &engine,
        &mut snaps,
        &serde_json::json!({ "path": "notes/r.md" }),
    )
    .expect("read");
    // The handler still returns the file bytes …
    assert!(reply["bytes"].is_array());
    // … and a snapshot is now available, keyed by the content TAG.
    let tag = nexus_hashline::tag("hello\n");
    assert_eq!(snaps.get_by_tag("notes/r.md", &tag).unwrap().content, "hello\n");
}

#[test]
fn edit_recovers_via_three_way_merge_after_external_change() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine.write_file("notes/m.md", b"a\nb\nc\nd\ne\n").expect("write");

    // Agent reads the file — records the base snapshot.
    let mut snaps = nexus_hashline::SnapshotStore::new();
    crate::handlers::files::read_file(
        &engine,
        &mut snaps,
        &serde_json::json!({ "path": "notes/m.md" }),
    )
    .expect("read");
    let base_tag = snaps.latest("notes/m.md").expect("snapshot").tag.clone();

    // The file changes underneath (line 4) before the agent's edit lands.
    engine
        .write_file("notes/m.md", b"a\nb\nc\nd-changed\ne\n")
        .expect("external change");

    // The agent edits line 2 against the now-stale base TAG.
    let patch = format!("[notes/m.md#{base_tag}]\nSWAP 2.=2:\n+b-edited\n");
    let reply = crate::handlers::files::edit_file(
        &engine,
        &snaps,
        &serde_json::json!({ "patch": patch }),
    )
    .expect("edit");

    assert_eq!(reply["files"][0]["status"], "merged");
    assert_eq!(reply["conflicts"].as_array().unwrap().len(), 0);
    assert_eq!(
        engine.read_file("notes/m.md").unwrap(),
        b"a\nb-edited\nc\nd-changed\ne\n"
    );
}

#[test]
fn edit_surfaces_conflict_without_writing() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");
    engine.write_file("notes/c.md", b"shared\n").expect("write");

    let mut snaps = nexus_hashline::SnapshotStore::new();
    crate::handlers::files::read_file(
        &engine,
        &mut snaps,
        &serde_json::json!({ "path": "notes/c.md" }),
    )
    .expect("read");
    let base_tag = snaps.latest("notes/c.md").expect("snapshot").tag.clone();

    // Both sides change the same single line — unresolvable.
    engine.write_file("notes/c.md", b"theirs\n").expect("external");
    let patch = format!("[notes/c.md#{base_tag}]\nSWAP 1.=1:\n+ours\n");
    let reply = crate::handlers::files::edit_file(
        &engine,
        &snaps,
        &serde_json::json!({ "patch": patch }),
    )
    .expect("edit");

    assert_eq!(reply["files"].as_array().unwrap().len(), 0);
    assert_eq!(reply["conflicts"].as_array().unwrap().len(), 1);
    assert_eq!(reply["conflicts"][0]["path"], "notes/c.md");
    // Conflict ⇒ nothing written; the external change stands.
    assert_eq!(engine.read_file("notes/c.md").unwrap(), b"theirs\n");
}
