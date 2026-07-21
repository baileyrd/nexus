//! PRD 03 smoke test: verifies the public API surface and end-to-end integration
//! for the nexus-storage crate.
//!
//! Each test exercises a distinct slice of the public contract.

use nexus_storage::{
    BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, RebuildStats, SearchIndex,
    SearchResult, StorageConfig, StorageEngine, StorageError, StorageEvent, TagResult,
};
use tempfile::TempDir;

fn tmp() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

// ── 1. public_type_surface_is_accessible ─────────────────────────────────────

/// Verify that all public types listed in the PRD can be imported and named.
#[test]
fn public_type_surface_is_accessible() {
    // StorageConfig
    let cfg = StorageConfig {
        pool_size: 2,
        debounce_ms: 100,
        rayon_threads: 0,
        defer_startup_reconcile: false,
    };
    assert_eq!(cfg.pool_size, 2);
    assert_eq!(cfg.debounce_ms, 100);

    // StorageConfig::default
    let dflt = StorageConfig::default();
    assert_eq!(dflt.pool_size, 4);
    assert_eq!(dflt.debounce_ms, 300);
    assert_eq!(dflt.rayon_threads, 0);

    // RebuildStats
    let stats = RebuildStats {
        files_processed: 1,
        blocks_indexed: 3,
        links_found: 0,
        tags_found: 1,
        duration_ms: 5,
    };
    assert_eq!(stats.files_processed, 1);

    // FileFilter
    let filter = FileFilter {
        prefix: Some("notes/".to_string()),
        file_type: None,
        include_deleted: false,
    };
    assert!(filter.prefix.is_some());

    // FileMetadata fields are accessible
    let meta = FileMetadata {
        path: "notes/a.md".to_string(),
        size_bytes: 42,
        modified_at: 0,
        content_hash: "abc".to_string(),
    };
    assert_eq!(meta.path, "notes/a.md");

    // StorageError variants
    let _e1: StorageError = StorageError::FileNotFound("x".into());
    let _e2: StorageError = StorageError::LockHeld("y".into());
    let _e3: StorageError = StorageError::ConfigInvalid("z".into());

    // Suppress unused-variable warnings
    let _ = (cfg, dflt, stats, filter, meta);
}

// ── 2. init_write_read_delete_lifecycle ───────────────────────────────────────

/// Full CRUD lifecycle: init → write → read → delete.
#[test]
fn init_write_read_delete_lifecycle() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    // Write
    let content = b"# Hello\n\nThis is a smoke-test note.";
    let meta = engine
        .write_file("notes/smoke.md", content)
        .expect("write_file");
    assert_eq!(meta.path, "notes/smoke.md");
    assert_eq!(meta.size_bytes, content.len() as u64);
    assert!(!meta.content_hash.is_empty());

    // Exists in index
    assert!(engine.file_exists("notes/smoke.md").expect("file_exists"));

    // Read back
    let read_back = engine.read_file("notes/smoke.md").expect("read_file");
    assert_eq!(read_back, content);

    // Delete
    engine.delete_file("notes/smoke.md").expect("delete_file");

    // No longer in index or on disk
    assert!(!engine.file_exists("notes/smoke.md").expect("file_exists"));
    assert!(!dir.path().join("notes/smoke.md").exists());
}

// ── 3. index_queries_return_parsed_data ──────────────────────────────────────

/// Write a file with frontmatter, tags, and wikilinks; verify each query API.
#[test]
fn index_queries_return_parsed_data() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    // First write the link target so the wikilink can be resolved.
    engine
        .write_file("notes/other.md", b"# Other\n\nTarget file.")
        .expect("write other");

    // Use the full path as the wikilink target so the link can be resolved to
    // a target_file_id (the parser sets target_path=None for bare [[name]] links
    // without a pipe; a piped link [[path|display]] is needed for resolution).
    let content =
        b"---\ntitle: Smoke Test\n---\n\n# Smoke Test\n\nThis uses #rust and [[notes/other.md|other]].\n";
    engine
        .write_file("notes/rich.md", content)
        .expect("write rich");

    // list_files
    let files = engine.list_files("notes/").expect("list_files");
    assert_eq!(files.len(), 2, "expected 2 notes, got {}", files.len());

    // query_files
    let filter = FileFilter {
        prefix: Some("notes/rich".to_string()),
        ..Default::default()
    };
    let records: Vec<FileRecord> = engine.query_files(&filter).expect("query_files");
    assert_eq!(records.len(), 1);
    let rich_id = records[0].id;

    // query_blocks
    let blocks: Vec<BlockRecord> = engine.query_blocks(rich_id).expect("query_blocks");
    assert!(!blocks.is_empty(), "expected at least 1 block");

    // query_links
    let links: Vec<LinkRecord> = engine.query_links(rich_id).expect("query_links");
    assert_eq!(links.len(), 1, "expected 1 outgoing link");
    assert_eq!(links[0].link_type, "wikilink");

    // query_backlinks for other.md
    let filter_other = FileFilter {
        prefix: Some("notes/other".to_string()),
        ..Default::default()
    };
    let other_records: Vec<FileRecord> = engine
        .query_files(&filter_other)
        .expect("query_files other");
    assert_eq!(other_records.len(), 1);
    let other_id = other_records[0].id;
    let backlinks: Vec<LinkRecord> = engine.query_backlinks(other_id).expect("query_backlinks");
    assert_eq!(backlinks.len(), 1, "expected 1 backlink to other.md");

    // query_tags
    let tags: Vec<TagResult> = engine.query_tags("rust").expect("query_tags");
    assert_eq!(tags.len(), 1, "expected 1 rust tag");
    assert_eq!(tags[0].file_path, "notes/rich.md");
}

// ── 4. search_index_standalone ────────────────────────────────────────────────

/// SearchIndex can be opened in-memory, populated, and searched independently.
#[test]
fn search_index_standalone() {
    let idx = SearchIndex::open_in_memory().expect("open_in_memory");

    idx.add_block(
        "notes/a.md",
        1,
        "paragraph",
        "rust programming is great",
        1_700_000_000,
    )
    .expect("add_block");
    idx.add_block(
        "notes/b.md",
        2,
        "heading",
        "introduction to rust",
        1_700_000_000,
    )
    .expect("add_block");
    idx.add_block(
        "notes/c.md",
        3,
        "paragraph",
        "python is also good",
        1_700_000_000,
    )
    .expect("add_block");
    idx.commit().expect("commit");

    let results: Vec<SearchResult> = idx.search("rust", 10).expect("search");
    assert_eq!(
        results.len(),
        2,
        "expected 2 rust matches, got {}",
        results.len()
    );

    for r in &results {
        assert!(
            r.file_path.starts_with("notes/"),
            "unexpected path: {}",
            r.file_path
        );
        assert!(r.score > 0.0, "score should be positive");
    }

    // No match
    let no_match = idx.search("haskell", 10).expect("search no match");
    assert!(no_match.is_empty());
}

// ── 5. rebuild_search_index_syncs_from_sqlite ─────────────────────────────────

/// After writing a file and calling rebuild_search_index, a search finds it.
#[test]
fn rebuild_search_index_syncs_from_sqlite() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    engine
        .write_file(
            "notes/tantivy.md",
            b"# Tantivy Note\n\nThis note is about fulltext search with tantivy.",
        )
        .expect("write");

    // Rebuild search index from SQLite.
    engine.rebuild_search_index().expect("rebuild_search_index");

    let results = engine.search("fulltext", 10).expect("search");
    assert!(
        !results.is_empty(),
        "expected at least 1 search result for 'fulltext'"
    );
    assert!(
        results.iter().any(|r| r.file_path == "notes/tantivy.md"),
        "expected notes/tantivy.md in results, got: {:?}",
        results.iter().map(|r| &r.file_path).collect::<Vec<_>>()
    );
}

// ── 6. parser_handles_complex_markdown ────────────────────────────────────────

/// parse_markdown handles heading, paragraph, code block, table, wikilink,
/// embed, and frontmatter without panicking or returning an error.
#[test]
fn parser_handles_complex_markdown() {
    use nexus_storage::parse_markdown;

    let complex = r#"---
title: Complex Note
tags: [rust, test]
---

# Heading One

A paragraph with **bold** and *italic* text.

```rust
fn main() { println!("hello"); }
```

| Col A | Col B |
|-------|-------|
| 1     | 2     |

This references [[other-note]] and embeds ![[image.png]].

#inline-tag at end.
"#;

    let parsed = parse_markdown(complex).expect("parse_markdown");

    // Frontmatter
    assert!(
        !parsed.frontmatter.is_empty(),
        "frontmatter should not be empty"
    );

    // Blocks
    assert!(!parsed.blocks.is_empty(), "blocks should not be empty");

    let has_heading = parsed
        .blocks
        .iter()
        .any(|b| b.block_type == "heading" && b.level == Some(1));
    assert!(has_heading, "expected at least one h1 heading block");

    // Links / embeds
    let wikilink = parsed.links.iter().find(|l| l.link_type == "wikilink");
    assert!(wikilink.is_some(), "expected at least one wikilink");

    let embed = parsed.links.iter().find(|l| l.link_type == "embed");
    assert!(embed.is_some(), "expected at least one embed link");

    // Tags
    let inline_tag = parsed.tags.iter().find(|t| t.name == "inline-tag");
    assert!(inline_tag.is_some(), "expected inline-tag in tags");

    // Content hash is populated
    assert!(!parsed.content_hash.is_empty());
}

// ── 7. storage_event_variants_construct ──────────────────────────────────────

/// All StorageEvent variants can be constructed and compared.
#[test]
fn storage_event_variants_construct() {
    let created = StorageEvent::FileCreated {
        path: "notes/a.md".to_string(),
        content_hash: "hash1".to_string(),
    };
    let modified = StorageEvent::FileModified {
        path: "notes/b.md".to_string(),
        content_hash: "hash2".to_string(),
    };
    let deleted = StorageEvent::FileDeleted {
        path: "notes/c.md".to_string(),
    };
    let renamed = StorageEvent::FileRenamed {
        from: "notes/old.md".to_string(),
        to: "notes/new.md".to_string(),
        content_hash: "hash3".to_string(),
    };

    // Equality
    assert_eq!(
        created,
        StorageEvent::FileCreated {
            path: "notes/a.md".to_string(),
            content_hash: "hash1".to_string(),
        }
    );

    // Inequality across variants
    assert_ne!(
        StorageEvent::FileCreated {
            path: "notes/a.md".to_string(),
            content_hash: "hash1".to_string(),
        },
        StorageEvent::FileDeleted {
            path: "notes/a.md".to_string(),
        }
    );

    // Debug formatting works
    let _ = format!("{created:?}");
    let _ = format!("{modified:?}");
    let _ = format!("{deleted:?}");
    let _ = format!("{renamed:?}");
}

// ── 8. reconcile_picks_up_external_changes ────────────────────────────────────

/// Write a file through the engine, then write a second file directly to disk,
/// and verify rebuild_index reports both files.
#[test]
fn reconcile_picks_up_external_changes() {
    let dir = tmp();
    let engine = StorageEngine::init(dir.path()).expect("init");

    // Write first file through the engine.
    engine
        .write_file(
            "notes/via-engine.md",
            b"# Via Engine\n\nWritten through API.",
        )
        .expect("write via engine");

    // Write second file directly to disk, bypassing the engine.
    let external_path = dir.path().join("notes").join("external.md");
    std::fs::write(&external_path, b"# External\n\nBypass write.").expect("write external");

    // rebuild_index should pick up both files.
    let stats = engine.rebuild_index().expect("rebuild_index");
    assert_eq!(
        stats.files_processed, 2,
        "expected 2 files_processed after external write, got {}",
        stats.files_processed
    );

    // Both should be findable via file_exists.
    assert!(engine
        .file_exists("notes/via-engine.md")
        .expect("file_exists via-engine"));
    assert!(engine
        .file_exists("notes/external.md")
        .expect("file_exists external"));
}
