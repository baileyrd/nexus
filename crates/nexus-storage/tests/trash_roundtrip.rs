//! C3 (#356) — engine-level trash: delete-to-trash removes an entry
//! from disk and the visible index, restore brings both back.

use nexus_storage::{DeleteDestination, StorageEngine};

fn write(engine: &StorageEngine, path: &str, content: &str) {
    engine
        .write_file(path, content.as_bytes())
        .expect("write_file");
}

#[test]
fn file_trash_restore_roundtrip_reindexes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    write(&engine, "notes/keep.md", "# Keep\n");
    write(&engine, "notes/gone.md", "# Gone\nsearchable-marker\n");

    let trash_id = engine
        .delete_entry_to("notes/gone.md", DeleteDestination::ForgeTrash)
        .expect("trash")
        .expect("forge trash returns a bucket id");

    assert!(!dir.path().join("notes/gone.md").exists());
    assert!(dir
        .path()
        .join(".trash")
        .join(&trash_id)
        .join("notes/gone.md")
        .exists());
    // Hidden from the root listing and present in trash_list.
    let names: Vec<String> = engine
        .list_dir("")
        .expect("list_dir")
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(!names.contains(&".trash".to_string()));
    let listed = engine.trash_list().expect("trash_list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].meta.original_path, "notes/gone.md");

    let restored = engine.trash_restore(&trash_id).expect("restore");
    assert_eq!(restored, "notes/gone.md");
    assert!(dir.path().join("notes/gone.md").exists());
    assert!(engine.trash_list().expect("trash_list").is_empty());

    // The restored note is fully re-indexed: a rename with link
    // rewriting can see it again via the links table.
    write(&engine, "notes/ref.md", "See [[gone]].\n");
    let (files, links) = engine
        .rename_entry_with_links("notes/gone.md", "notes/found.md", true)
        .expect("rename");
    assert_eq!((files, links), (1, 1));
}

#[test]
fn directory_trash_soft_deletes_and_restores_children() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");

    write(&engine, "proj/a.md", "# A\n");
    write(&engine, "proj/data.csv", "x,y\n1,2\n");

    let trash_id = engine
        .delete_entry_to("proj", DeleteDestination::ForgeTrash)
        .expect("trash dir")
        .expect("bucket id");
    assert!(!dir.path().join("proj").exists());

    let restored = engine.trash_restore(&trash_id).expect("restore dir");
    assert_eq!(restored, "proj");
    assert!(dir.path().join("proj/a.md").exists());
    assert!(dir.path().join("proj/data.csv").exists());
}

#[test]
fn trash_refuses_forge_internals() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");
    for path in [".forge", ".trash", "", ".forge/index.db"] {
        assert!(
            engine
                .delete_entry_to(path, DeleteDestination::ForgeTrash)
                .is_err(),
            "must refuse to trash '{path}'"
        );
    }
}

#[test]
fn trash_empty_removes_buckets_permanently() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = StorageEngine::init(dir.path()).expect("init forge");
    write(&engine, "a.md", "# A\n");
    write(&engine, "b.md", "# B\n");
    engine
        .delete_entry_to("a.md", DeleteDestination::ForgeTrash)
        .expect("trash a");
    engine
        .delete_entry_to("b.md", DeleteDestination::ForgeTrash)
        .expect("trash b");
    assert_eq!(engine.trash_list().expect("list").len(), 2);
    // Nothing is older than 1 day.
    assert_eq!(engine.trash_empty(Some(1)).expect("empty aged"), 0);
    assert_eq!(engine.trash_empty(None).expect("empty all"), 2);
    assert!(engine.trash_list().expect("list").is_empty());
}
