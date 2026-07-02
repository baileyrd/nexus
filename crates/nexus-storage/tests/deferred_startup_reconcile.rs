//! C18 (#370) — `defer_startup_reconcile` opens the engine without the
//! synchronous reconcile; `reconcile_index_full` later brings both the
//! SQLite index and the in-memory graph up to date.

use nexus_storage::{FileFilter, StorageConfig, StorageEngine};

fn indexed(engine: &StorageEngine, path: &str) -> bool {
    engine
        .query_files(&FileFilter {
            prefix: Some(path.to_string()),
            ..FileFilter::default()
        })
        .expect("query_files")
        .iter()
        .any(|f| f.path == path)
}

#[test]
fn deferred_open_skips_reconcile_and_full_pass_catches_up() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Seed a forge, then add a file while the engine is "not running".
    {
        let engine = StorageEngine::init(dir.path()).expect("init forge");
        engine.write_file("notes/a.md", b"# A\n").expect("seed");
    }
    std::fs::write(dir.path().join("notes/offline.md"), "# Offline\n").unwrap();

    // Deferred open: the offline file must NOT be indexed yet.
    let config = StorageConfig {
        defer_startup_reconcile: true,
        ..StorageConfig::default()
    };
    let engine = StorageEngine::open(dir.path(), &config).expect("deferred open");
    assert!(
        !indexed(&engine, "notes/offline.md"),
        "deferred open must not have indexed the offline-added file"
    );

    // The background pass (simulated synchronously here) catches up.
    let delta = engine.reconcile_index_full().expect("full reconcile");
    assert_eq!(delta.created, 1, "exactly the offline file is new");
    assert!(
        indexed(&engine, "notes/offline.md"),
        "reconcile_index_full must index the offline-added file"
    );
}

#[test]
fn blocking_open_still_reconciles_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let engine = StorageEngine::init(dir.path()).expect("init forge");
        engine.write_file("notes/a.md", b"# A\n").expect("seed");
    }
    std::fs::write(dir.path().join("notes/offline.md"), "# Offline\n").unwrap();

    let engine =
        StorageEngine::open(dir.path(), &StorageConfig::default()).expect("blocking open");
    assert!(
        indexed(&engine, "notes/offline.md"),
        "default open must keep the pre-C18 synchronous reconcile"
    );
}
