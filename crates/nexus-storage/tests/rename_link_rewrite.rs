//! C2 (#355) — renaming a note rewrites inbound links in referencing
//! files when `update_links` is on, and leaves them untouched when off.
//!
//! End-to-end at the engine layer: referencing files are discovered
//! through the `links` table the indexer populated, rewritten on disk,
//! and re-persisted through `write_file` so their own index rows
//! refresh.

use nexus_storage::StorageEngine;

fn write(engine: &StorageEngine, path: &str, content: &str) {
    engine
        .write_file(path, content.as_bytes())
        .expect("write_file");
}

fn read(dir: &std::path::Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).expect("read back")
}

#[test]
fn rename_with_update_links_rewrites_referencing_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    // Target first so the referrer's links resolve at index time.
    write(&engine, "notes/Target.md", "# Target\n");
    write(
        &engine,
        "notes/Referrer.md",
        "See [[Target]], [[Target|the target]], and [md](notes/Target.md).\n",
    );
    write(&engine, "notes/Unrelated.md", "No links here. [[Other]]\n");

    let (files, links) = engine
        .rename_entry_with_links("notes/Target.md", "notes/Renamed.md", true)
        .expect("rename with link update");

    assert_eq!(files, 1, "only the referencing file is rewritten");
    assert_eq!(links, 3, "all three link forms update");
    assert_eq!(
        read(dir.path(), "notes/Referrer.md"),
        "See [[Renamed]], [[Renamed|the target]], and [md](notes/Renamed.md).\n",
    );
    assert_eq!(
        read(dir.path(), "notes/Unrelated.md"),
        "No links here. [[Other]]\n",
        "files that never referenced the target stay byte-identical",
    );
    assert!(dir.path().join("notes/Renamed.md").exists());
    assert!(!dir.path().join("notes/Target.md").exists());
}

#[test]
fn rename_without_update_links_keeps_old_behaviour() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    write(&engine, "notes/Target.md", "# Target\n");
    write(&engine, "notes/Referrer.md", "See [[Target]].\n");

    let (files, links) = engine
        .rename_entry_with_links("notes/Target.md", "notes/Renamed.md", false)
        .expect("rename without link update");

    assert_eq!((files, links), (0, 0));
    assert_eq!(
        read(dir.path(), "notes/Referrer.md"),
        "See [[Target]].\n",
        "update_links=false must not touch referencing files",
    );
}

#[test]
fn rewritten_referrer_reindexes_against_the_new_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    write(&engine, "notes/Target.md", "# Target\n");
    write(&engine, "notes/Referrer.md", "See [[Target]].\n");

    engine
        .rename_entry_with_links("notes/Target.md", "notes/Renamed.md", true)
        .expect("rename");

    // A second rename must find the referrer again via the refreshed
    // links table — proving the rewrite went through write_file's
    // reindex rather than a bare fs write.
    let (files, links) = engine
        .rename_entry_with_links("notes/Renamed.md", "notes/Final.md", true)
        .expect("second rename");
    assert_eq!((files, links), (1, 1));
    assert_eq!(read(dir.path(), "notes/Referrer.md"), "See [[Final]].\n");
}
