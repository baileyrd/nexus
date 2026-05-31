//! Growth Plan Phase 1 smoke tests -- knowledge graph + backlinks.

use nexus_storage::StorageEngine;

fn engine() -> (tempfile::TempDir, StorageEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(dir.path()).unwrap();
    (dir, engine)
}

#[test]
fn backlinks_and_outgoing_links() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"# A\n\nLink to [[notes/b.md]]\n")
        .unwrap();
    engine
        .write_file("notes/b.md", b"# B\n\nLink to [[notes/a.md]]\n")
        .unwrap();
    engine
        .write_file("notes/c.md", b"# C\n\nLink to [[notes/b.md]]\n")
        .unwrap();

    let bl = engine.backlinks("notes/b.md").unwrap();
    assert_eq!(bl.len(), 2, "b should have 2 backlinks, got {}", bl.len());
    let sources: Vec<&str> = bl.iter().map(|b| b.source_path.as_str()).collect();
    assert!(sources.contains(&"notes/a.md"));
    assert!(sources.contains(&"notes/c.md"));

    let out = engine.outgoing_links("notes/a.md").unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].target_path, "notes/b.md");
    assert!(out[0].is_resolved);
}

#[test]
fn unresolved_links_detected() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"See [[notes/missing.md]]\n")
        .unwrap();

    let unresolved = engine.unresolved_links().unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].target_path, "notes/missing.md");
    assert!(unresolved[0]
        .referenced_by
        .contains(&"notes/a.md".to_string()));
}

#[test]
fn adding_file_resolves_phantom() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"See [[notes/b.md]]\n")
        .unwrap();
    assert_eq!(engine.unresolved_links().unwrap().len(), 1);

    engine.write_file("notes/b.md", b"# B\n").unwrap();
    assert_eq!(engine.unresolved_links().unwrap().len(), 0);

    let bl = engine.backlinks("notes/b.md").unwrap();
    assert_eq!(bl.len(), 1);
}

#[test]
fn deleting_file_updates_graph() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"Link to [[notes/b.md]]\n")
        .unwrap();
    engine.write_file("notes/b.md", b"# B\n").unwrap();

    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 1);

    engine.delete_file("notes/a.md").unwrap();

    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 0);
}

#[test]
fn graph_stats_correct() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"[[notes/b.md]] and [[notes/c.md]]\n")
        .unwrap();
    engine.write_file("notes/b.md", b"# B\n").unwrap();

    let stats = engine.graph_stats().unwrap();
    assert_eq!(stats.node_count, 3); // a, b, c(phantom)
    assert_eq!(stats.edge_count, 2);
    assert_eq!(stats.unresolved_count, 1);
}

#[test]
fn graph_neighbors_traversal() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"[[notes/b.md]]\n")
        .unwrap();
    engine
        .write_file("notes/b.md", b"[[notes/c.md]]\n")
        .unwrap();
    engine.write_file("notes/c.md", b"# C\n").unwrap();

    let n1 = engine.graph_neighbors("notes/a.md", 1).unwrap();
    assert_eq!(n1.len(), 1);
    assert!(n1.contains(&"notes/b.md".to_string()));

    let n2 = engine.graph_neighbors("notes/a.md", 2).unwrap();
    assert_eq!(n2.len(), 2);
    assert!(n2.contains(&"notes/b.md".to_string()));
    assert!(n2.contains(&"notes/c.md".to_string()));
}

#[test]
fn rewrite_file_updates_graph() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"[[notes/b.md]]\n")
        .unwrap();
    engine.write_file("notes/b.md", b"# B\n").unwrap();

    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 1);

    engine
        .write_file("notes/a.md", b"# A\n\nNo links here.\n")
        .unwrap();

    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 0);
}
