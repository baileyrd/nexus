//! C17 (#371) — external (non-engine) file changes reach the index via
//! `index_external_change` / `index_external_delete`, with echo
//! suppression for engine-initiated writes.

use nexus_storage::StorageEngine;

#[test]
fn external_create_and_modify_are_indexed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    // Simulate vim: write straight to disk, bypassing the engine.
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    std::fs::write(dir.path().join("notes/ext.md"), "# Ext\nSee [[Other]].\n").unwrap();

    let meta = engine
        .index_external_change("notes/ext.md")
        .expect("index external create")
        .expect("newly created file must be indexed");
    assert_eq!(meta.path, "notes/ext.md");

    // The link written outside the engine is now in the links table:
    // renaming its target rewrites the externally-authored file.
    engine.write_file("notes/Other.md", b"# Other\n").unwrap();
    std::fs::write(
        dir.path().join("notes/ext.md"),
        "# Ext\nSee [[Other]] twice: [[Other]].\n",
    )
    .unwrap();
    engine
        .index_external_change("notes/ext.md")
        .expect("index external modify")
        .expect("changed content must re-index");

    let (files, links) = engine
        .rename_entry_with_links("notes/Other.md", "notes/Renamed.md", true)
        .expect("rename");
    assert_eq!(files, 1);
    assert_eq!(links, 2);
    let rewritten = std::fs::read_to_string(dir.path().join("notes/ext.md")).unwrap();
    assert!(rewritten.contains("[[Renamed]]"));
}

#[test]
fn engine_writes_are_echo_suppressed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    engine.write_file("a.md", b"# A\n").unwrap();
    // The watcher would now deliver FileCreated for a.md; the bridge's
    // index update must be a no-op because the hash already matches.
    let result = engine.index_external_change("a.md").expect("echo check");
    assert!(result.is_none(), "unchanged content must not re-index");
}

#[test]
fn binary_files_are_skipped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");
    std::fs::write(dir.path().join("blob.png"), [0u8, 159, 146, 150]).unwrap();
    let result = engine
        .index_external_change("blob.png")
        .expect("binary skip");
    assert!(result.is_none());
}

#[test]
fn vanished_files_are_skipped_not_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");
    let result = engine
        .index_external_change("never-existed.md")
        .expect("missing file is a skip");
    assert!(result.is_none());
}

#[test]
fn external_delete_soft_deletes_and_reconcile_resurrects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    engine.write_file("notes/x.md", b"# X\n").unwrap();
    std::fs::remove_file(dir.path().join("notes/x.md")).unwrap();
    engine
        .index_external_delete("notes/x.md")
        .expect("external delete");

    // Rewriting the file at the same path must not trip the UNIQUE
    // path slot the soft-deleted row still occupies.
    engine.write_file("notes/x.md", b"# X again\n").unwrap();
}
