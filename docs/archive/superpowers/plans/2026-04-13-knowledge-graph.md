# Knowledge Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a live in-memory knowledge graph with backlink queries, unresolved link detection, and BFS neighbor traversal exposed through the StorageEngine API and CLI.

**Architecture:** A `KnowledgeGraph` struct wrapping `petgraph::StableGraph` lives inside `StorageEngine` behind `Arc<RwLock<>>`. It's built from the SQLite `links` table on startup and updated incrementally on every `write_file`/`delete_file`. CLI commands in a new `graph` command group expose the queries.

**Tech Stack:** petgraph 0.7 (new), rusqlite (existing), clap (existing)

---

## File Structure

| File | Role | Change |
|------|------|--------|
| `Cargo.toml` (workspace) | Workspace deps | Add `petgraph = "0.7"` |
| `crates/nexus-storage/Cargo.toml` | Crate deps | Add `petgraph = { workspace = true }` |
| `crates/nexus-storage/src/graph.rs` | KnowledgeGraph struct + all types | **NEW** |
| `crates/nexus-storage/src/lib.rs` | StorageEngine integration | Add graph field, methods, update write/delete |
| `crates/nexus-cli/src/main.rs` | CLI arg parser | Add Graph command group + content links/backlinks |
| `crates/nexus-cli/src/commands/graph.rs` | Graph CLI handlers | **NEW** |
| `crates/nexus-cli/src/commands/content.rs` | Content CLI handlers | Add links/backlinks |
| `crates/nexus-cli/src/commands/mod.rs` | Module registry | Add graph |
| `crates/nexus-storage/tests/prd-06-phase1-smoke.rs` | Integration tests | **NEW** |

---

### Task 1: Add petgraph Dependency

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/nexus-storage/Cargo.toml`

- [ ] **Step 1: Add petgraph to workspace dependencies**

In the workspace root `Cargo.toml`, add under `[workspace.dependencies]`:

```toml
# Knowledge graph
petgraph = "0.7"
```

- [ ] **Step 2: Add petgraph to nexus-storage**

In `crates/nexus-storage/Cargo.toml`, add under `[dependencies]`:

```toml
petgraph = { workspace = true }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p nexus-storage`
Expected: PASS (petgraph resolves and compiles)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/nexus-storage/Cargo.toml
git commit -m "chore(storage): add petgraph dependency for knowledge graph"
```

---

### Task 2: KnowledgeGraph Core — Types and Construction

**Files:**
- Create: `crates/nexus-storage/src/graph.rs`
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Create graph.rs with types and empty struct**

Create `crates/nexus-storage/src/graph.rs`:

```rust
//! In-memory knowledge graph built on petgraph.
//!
//! Represents notes as nodes and links as directed edges.
//! Rebuilt from SQLite on startup, updated incrementally on file changes.

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::Direction;
use rusqlite::Connection;

use crate::StorageError;

// ── Node and Edge data ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct NodeData {
    path: String,
}

/// Metadata stored on each directed edge in the knowledge graph.
#[derive(Debug, Clone)]
pub struct EdgeData {
    /// Kind of link: "wikilink", "markdown", or "embed".
    pub link_type: String,
    /// Display text of the link.
    pub link_text: String,
    /// Fragment identifier (e.g. "Heading" or "^blockid").
    pub fragment: Option<String>,
}

// ── Query result types ───────────────────────────────────────────────────────

/// A file that links TO the queried path.
#[derive(Debug, Clone)]
pub struct BacklinkResult {
    /// Path of the file containing the link.
    pub source_path: String,
    /// Display text of the link.
    pub link_text: String,
    /// Kind of link.
    pub link_type: String,
}

/// A link FROM the queried path to another file.
#[derive(Debug, Clone)]
pub struct OutgoingLink {
    /// Path of the link target.
    pub target_path: String,
    /// Display text of the link.
    pub link_text: String,
    /// Kind of link.
    pub link_type: String,
    /// Whether the target file exists in the forge.
    pub is_resolved: bool,
    /// Fragment identifier, if any.
    pub fragment: Option<String>,
}

/// A link target that has no corresponding file.
#[derive(Debug, Clone)]
pub struct UnresolvedLink {
    /// The missing target path.
    pub target_path: String,
    /// Paths of files that reference this target.
    pub referenced_by: Vec<String>,
}

/// Summary statistics for the knowledge graph.
#[derive(Debug, Clone)]
pub struct GraphStats {
    /// Total number of nodes (files + phantoms).
    pub node_count: usize,
    /// Total number of directed edges.
    pub edge_count: usize,
    /// Number of phantom (unresolved) nodes.
    pub unresolved_count: usize,
}

// ── KnowledgeGraph ───────────────────────────────────────────────────────────

/// In-memory directed graph of notes and links.
pub struct KnowledgeGraph {
    graph: StableGraph<NodeData, EdgeData>,
    path_to_node: HashMap<String, NodeIndex>,
    phantom_nodes: HashSet<NodeIndex>,
}

impl KnowledgeGraph {
    /// Create an empty knowledge graph.
    pub fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            path_to_node: HashMap::new(),
            phantom_nodes: HashSet::new(),
        }
    }
}
```

- [ ] **Step 2: Register the module in lib.rs**

In `crates/nexus-storage/src/lib.rs`, add after `mod tasks;`:

```rust
mod graph;
```

Add to the pub use exports:

```rust
pub use graph::{KnowledgeGraph, BacklinkResult, OutgoingLink, UnresolvedLink, GraphStats, EdgeData};
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p nexus-storage`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-storage/src/graph.rs crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): add KnowledgeGraph struct with types"
```

---

### Task 3: KnowledgeGraph — Mutation Methods

**Files:**
- Modify: `crates/nexus-storage/src/graph.rs`

- [ ] **Step 1: Write failing tests for mutation methods**

Add to `graph.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_note_is_idempotent() {
        let mut kg = KnowledgeGraph::new();
        let idx1 = kg.add_note("notes/a.md");
        let idx2 = kg.add_note("notes/a.md");
        assert_eq!(idx1, idx2);
        assert_eq!(kg.stats().node_count, 1);
    }

    #[test]
    fn add_link_creates_edge() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_note("notes/b.md");
        kg.add_link("notes/a.md", "notes/b.md", EdgeData {
            link_type: "wikilink".to_string(),
            link_text: "b".to_string(),
            fragment: None,
        });
        assert_eq!(kg.stats().edge_count, 1);
    }

    #[test]
    fn add_link_creates_phantom_for_missing_target() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_link("notes/a.md", "notes/missing.md", EdgeData {
            link_type: "wikilink".to_string(),
            link_text: "missing".to_string(),
            fragment: None,
        });
        assert_eq!(kg.stats().node_count, 2);
        assert_eq!(kg.stats().unresolved_count, 1);
    }

    #[test]
    fn add_note_promotes_phantom() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_link("notes/a.md", "notes/b.md", EdgeData {
            link_type: "wikilink".to_string(),
            link_text: "b".to_string(),
            fragment: None,
        });
        assert_eq!(kg.stats().unresolved_count, 1);

        // Adding the file promotes the phantom
        kg.add_note("notes/b.md");
        assert_eq!(kg.stats().unresolved_count, 0);
        assert_eq!(kg.stats().node_count, 2);
    }

    #[test]
    fn remove_note_removes_edges() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_note("notes/b.md");
        kg.add_link("notes/a.md", "notes/b.md", EdgeData {
            link_type: "wikilink".to_string(),
            link_text: "b".to_string(),
            fragment: None,
        });
        kg.remove_note("notes/a.md");
        assert_eq!(kg.stats().node_count, 1);
        assert_eq!(kg.stats().edge_count, 0);
    }

    #[test]
    fn remove_links_from_clears_outgoing() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_note("notes/b.md");
        kg.add_note("notes/c.md");
        kg.add_link("notes/a.md", "notes/b.md", EdgeData {
            link_type: "wikilink".to_string(),
            link_text: "b".to_string(),
            fragment: None,
        });
        kg.add_link("notes/a.md", "notes/c.md", EdgeData {
            link_type: "wikilink".to_string(),
            link_text: "c".to_string(),
            fragment: None,
        });
        assert_eq!(kg.stats().edge_count, 2);

        kg.remove_links_from("notes/a.md");
        assert_eq!(kg.stats().edge_count, 0);
        // Node still exists
        assert_eq!(kg.stats().node_count, 3);
    }

    #[test]
    fn empty_graph_stats() {
        let kg = KnowledgeGraph::new();
        let s = kg.stats();
        assert_eq!(s.node_count, 0);
        assert_eq!(s.edge_count, 0);
        assert_eq!(s.unresolved_count, 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexus-storage --lib graph`
Expected: FAIL — methods don't exist

- [ ] **Step 3: Implement mutation methods**

Add to `impl KnowledgeGraph`:

```rust
/// Add a note to the graph. Idempotent — returns existing index if present.
/// If the path was a phantom node, promotes it to real.
pub fn add_note(&mut self, path: &str) -> NodeIndex {
    if let Some(&idx) = self.path_to_node.get(path) {
        // Promote from phantom if needed
        self.phantom_nodes.remove(&idx);
        return idx;
    }
    let idx = self.graph.add_node(NodeData { path: path.to_string() });
    self.path_to_node.insert(path.to_string(), idx);
    idx
}

/// Remove a note and all its incident edges from the graph.
pub fn remove_note(&mut self, path: &str) {
    if let Some(idx) = self.path_to_node.remove(path) {
        self.graph.remove_node(idx);
        self.phantom_nodes.remove(&idx);
    }
}

/// Add a directed link from `source` to `target`.
/// Creates a phantom node for `target` if it doesn't exist.
pub fn add_link(&mut self, source: &str, target: &str, edge: EdgeData) {
    let src_idx = self.add_note(source);
    let tgt_idx = if let Some(&idx) = self.path_to_node.get(target) {
        idx
    } else {
        // Create phantom node for unresolved target
        let idx = self.graph.add_node(NodeData { path: target.to_string() });
        self.path_to_node.insert(target.to_string(), idx);
        self.phantom_nodes.insert(idx);
        idx
    };
    self.graph.add_edge(src_idx, tgt_idx, edge);
}

/// Remove all outgoing edges from `source`, keeping the node.
pub fn remove_links_from(&mut self, source: &str) {
    if let Some(&idx) = self.path_to_node.get(source) {
        let outgoing: Vec<_> = self.graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|e| e.id())
            .collect();
        for edge_id in outgoing {
            self.graph.remove_edge(edge_id);
        }
    }
}

/// Return graph statistics.
pub fn stats(&self) -> GraphStats {
    GraphStats {
        node_count: self.graph.node_count(),
        edge_count: self.graph.edge_count(),
        unresolved_count: self.phantom_nodes.len(),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexus-storage --lib graph`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/src/graph.rs
git commit -m "feat(storage): implement KnowledgeGraph mutation methods"
```

---

### Task 4: KnowledgeGraph — Query Methods

**Files:**
- Modify: `crates/nexus-storage/src/graph.rs`

- [ ] **Step 1: Write failing tests for query methods**

Add to the `tests` module:

```rust
#[test]
fn backlinks_returns_incoming() {
    let mut kg = KnowledgeGraph::new();
    kg.add_note("notes/a.md");
    kg.add_note("notes/b.md");
    kg.add_link("notes/a.md", "notes/b.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "b".to_string(),
        fragment: None,
    });
    let bl = kg.backlinks("notes/b.md");
    assert_eq!(bl.len(), 1);
    assert_eq!(bl[0].source_path, "notes/a.md");
    assert_eq!(bl[0].link_text, "b");
}

#[test]
fn backlinks_empty_for_no_incoming() {
    let mut kg = KnowledgeGraph::new();
    kg.add_note("notes/a.md");
    let bl = kg.backlinks("notes/a.md");
    assert!(bl.is_empty());
}

#[test]
fn outgoing_links_shows_resolved_status() {
    let mut kg = KnowledgeGraph::new();
    kg.add_note("notes/a.md");
    kg.add_note("notes/b.md");
    kg.add_link("notes/a.md", "notes/b.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "b".to_string(),
        fragment: None,
    });
    kg.add_link("notes/a.md", "notes/missing.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "missing".to_string(),
        fragment: Some("heading".to_string()),
    });
    let out = kg.outgoing_links("notes/a.md");
    assert_eq!(out.len(), 2);

    let resolved = out.iter().find(|l| l.target_path == "notes/b.md").unwrap();
    assert!(resolved.is_resolved);

    let unresolved = out.iter().find(|l| l.target_path == "notes/missing.md").unwrap();
    assert!(!unresolved.is_resolved);
    assert_eq!(unresolved.fragment, Some("heading".to_string()));
}

#[test]
fn unresolved_links_lists_phantoms() {
    let mut kg = KnowledgeGraph::new();
    kg.add_note("notes/a.md");
    kg.add_note("notes/b.md");
    kg.add_link("notes/a.md", "notes/missing1.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "m1".to_string(),
        fragment: None,
    });
    kg.add_link("notes/b.md", "notes/missing1.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "m1".to_string(),
        fragment: None,
    });
    kg.add_link("notes/a.md", "notes/missing2.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "m2".to_string(),
        fragment: None,
    });
    let unresolved = kg.unresolved_links();
    assert_eq!(unresolved.len(), 2);

    let m1 = unresolved.iter().find(|u| u.target_path == "notes/missing1.md").unwrap();
    assert_eq!(m1.referenced_by.len(), 2);
}

#[test]
fn neighbors_bfs_depth_1() {
    let mut kg = KnowledgeGraph::new();
    kg.add_note("notes/a.md");
    kg.add_note("notes/b.md");
    kg.add_note("notes/c.md");
    kg.add_link("notes/a.md", "notes/b.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "b".to_string(),
        fragment: None,
    });
    kg.add_link("notes/b.md", "notes/c.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "c".to_string(),
        fragment: None,
    });
    let n = kg.neighbors("notes/a.md", 1);
    assert_eq!(n.len(), 1);
    assert!(n.contains(&"notes/b.md".to_string()));
}

#[test]
fn neighbors_bfs_depth_2() {
    let mut kg = KnowledgeGraph::new();
    kg.add_note("notes/a.md");
    kg.add_note("notes/b.md");
    kg.add_note("notes/c.md");
    kg.add_link("notes/a.md", "notes/b.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "b".to_string(),
        fragment: None,
    });
    kg.add_link("notes/b.md", "notes/c.md", EdgeData {
        link_type: "wikilink".to_string(),
        link_text: "c".to_string(),
        fragment: None,
    });
    let n = kg.neighbors("notes/a.md", 2);
    assert_eq!(n.len(), 2);
    assert!(n.contains(&"notes/b.md".to_string()));
    assert!(n.contains(&"notes/c.md".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexus-storage --lib graph`
Expected: FAIL — methods don't exist

- [ ] **Step 3: Implement query methods**

Add to `impl KnowledgeGraph`:

```rust
/// Return all files that link TO `path`.
pub fn backlinks(&self, path: &str) -> Vec<BacklinkResult> {
    let Some(&idx) = self.path_to_node.get(path) else {
        return Vec::new();
    };
    self.graph
        .edges_directed(idx, Direction::Incoming)
        .map(|edge| {
            let source_idx = edge.source();
            BacklinkResult {
                source_path: self.graph[source_idx].path.clone(),
                link_text: edge.weight().link_text.clone(),
                link_type: edge.weight().link_type.clone(),
            }
        })
        .collect()
}

/// Return all links FROM `path` to other files.
pub fn outgoing_links(&self, path: &str) -> Vec<OutgoingLink> {
    let Some(&idx) = self.path_to_node.get(path) else {
        return Vec::new();
    };
    self.graph
        .edges_directed(idx, Direction::Outgoing)
        .map(|edge| {
            let target_idx = edge.target();
            OutgoingLink {
                target_path: self.graph[target_idx].path.clone(),
                link_text: edge.weight().link_text.clone(),
                link_type: edge.weight().link_type.clone(),
                is_resolved: !self.phantom_nodes.contains(&target_idx),
                fragment: edge.weight().fragment.clone(),
            }
        })
        .collect()
}

/// Return all phantom nodes (unresolved link targets) with their referrers.
pub fn unresolved_links(&self) -> Vec<UnresolvedLink> {
    self.phantom_nodes
        .iter()
        .map(|&idx| {
            let target_path = self.graph[idx].path.clone();
            let referenced_by: Vec<String> = self.graph
                .edges_directed(idx, Direction::Incoming)
                .map(|e| self.graph[e.source()].path.clone())
                .collect();
            UnresolvedLink { target_path, referenced_by }
        })
        .collect()
}

/// BFS traversal from `path` up to `depth` hops in both directions.
/// Returns unique paths excluding the start node.
pub fn neighbors(&self, path: &str, depth: usize) -> Vec<String> {
    let Some(&start) = self.path_to_node.get(path) else {
        return Vec::new();
    };

    let mut visited = HashSet::new();
    visited.insert(start);
    let mut queue = VecDeque::new();
    queue.push_back((start, 0usize));
    let mut result = Vec::new();

    while let Some((node, d)) = queue.pop_front() {
        if d >= depth {
            continue;
        }
        // Traverse both directions
        for direction in [Direction::Outgoing, Direction::Incoming] {
            for neighbor in self.graph.neighbors_directed(node, direction) {
                if visited.insert(neighbor) {
                    result.push(self.graph[neighbor].path.clone());
                    queue.push_back((neighbor, d + 1));
                }
            }
        }
    }

    result
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexus-storage --lib graph`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/src/graph.rs
git commit -m "feat(storage): implement KnowledgeGraph query methods — backlinks, outgoing, unresolved, neighbors"
```

---

### Task 5: KnowledgeGraph — rebuild_from_db

**Files:**
- Modify: `crates/nexus-storage/src/graph.rs`

- [ ] **Step 1: Write failing test**

Add to the `tests` module:

```rust
#[test]
fn rebuild_from_db() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::schema::configure_pragmas(&conn).unwrap();
    crate::schema::migrate(&conn).unwrap();

    // Insert two files
    conn.execute(
        "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
         VALUES ('notes/a.md', 'markdown', 'h1', 10, 0, 0);",
        [],
    ).unwrap();
    let a_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
         VALUES ('notes/b.md', 'markdown', 'h2', 10, 0, 0);",
        [],
    ).unwrap();
    let b_id = conn.last_insert_rowid();

    // a links to b (resolved)
    conn.execute(
        "INSERT INTO links (source_file_id, target_path, target_file_id, link_text, link_type, is_resolved)
         VALUES (?1, 'notes/b.md', ?2, 'b', 'wikilink', 1);",
        rusqlite::params![a_id, b_id],
    ).unwrap();

    // a links to missing (unresolved)
    conn.execute(
        "INSERT INTO links (source_file_id, target_path, link_text, link_type, is_resolved)
         VALUES (?1, 'notes/missing.md', 'missing', 'wikilink', 0);",
        rusqlite::params![a_id],
    ).unwrap();

    let kg = KnowledgeGraph::rebuild_from_db(&conn).unwrap();

    let stats = kg.stats();
    assert_eq!(stats.node_count, 3); // a, b, missing(phantom)
    assert_eq!(stats.edge_count, 2);
    assert_eq!(stats.unresolved_count, 1);

    let bl = kg.backlinks("notes/b.md");
    assert_eq!(bl.len(), 1);
    assert_eq!(bl[0].source_path, "notes/a.md");

    let unresolved = kg.unresolved_links();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].target_path, "notes/missing.md");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexus-storage --lib rebuild_from_db`
Expected: FAIL — method doesn't exist

- [ ] **Step 3: Implement rebuild_from_db**

Add to `impl KnowledgeGraph`:

```rust
/// Build the knowledge graph from the SQLite index.
///
/// Queries all non-deleted files as nodes and all links as edges.
/// Unresolved link targets become phantom nodes.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any SQLite failure.
pub fn rebuild_from_db(conn: &Connection) -> Result<Self, StorageError> {
    let mut kg = Self::new();

    // 1. Add all files as nodes
    let mut stmt = conn.prepare(
        "SELECT path FROM files WHERE is_deleted = 0;"
    )?;
    let paths: Vec<String> = stmt.query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for path in &paths {
        kg.add_note(path);
    }

    // 2. Add all links as edges
    let mut stmt = conn.prepare(
        "SELECT f.path, l.target_path, l.link_text, l.link_type, l.fragment
         FROM links l
         JOIN files f ON f.id = l.source_file_id
         WHERE f.is_deleted = 0 AND l.target_path IS NOT NULL;"
    )?;
    let links: Vec<(String, String, String, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (source, target, link_text, link_type, fragment) in links {
        kg.add_link(&source, &target, EdgeData {
            link_type,
            link_text,
            fragment,
        });
    }

    Ok(kg)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexus-storage --lib graph`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/src/graph.rs
git commit -m "feat(storage): implement KnowledgeGraph::rebuild_from_db"
```

---

### Task 6: StorageEngine Integration

**Files:**
- Modify: `crates/nexus-storage/src/lib.rs`

- [ ] **Step 1: Add graph field to StorageEngine**

Add imports at the top of `lib.rs`:

```rust
use std::sync::{Arc, RwLock};
```

Note: `Mutex` is already imported. `Arc` and `RwLock` need to be added.

Update the `StorageEngine` struct:

```rust
pub struct StorageEngine {
    forge: Forge,
    _lock: ForgeLock,
    pool: r2d2::Pool<SqliteConnectionManager>,
    write_conn: Mutex<rusqlite::Connection>,
    search_index: SearchIndex,
    watcher: Option<watcher::Watcher>,
    graph: Arc<RwLock<graph::KnowledgeGraph>>,
}
```

- [ ] **Step 2: Initialize graph in open_internal**

In `open_internal`, after step 8 (reconcile), add:

```rust
// 9. Build knowledge graph from DB.
let kg = if is_new {
    graph::KnowledgeGraph::new()
} else {
    graph::KnowledgeGraph::rebuild_from_db(&write_conn)?
};
let graph = Arc::new(RwLock::new(kg));
```

Update the `Ok(StorageEngine { ... })` return to include `graph`.

- [ ] **Step 3: Update write_file to maintain graph**

After step 5 (`insert_file`) in `write_file`, add graph update:

```rust
// 7. Update knowledge graph.
{
    let mut g = self.graph.write().expect("graph lock poisoned");
    g.add_note(path);
    g.remove_links_from(path);
    for link in &parsed.links {
        let target = link.target_path.as_deref()
            .unwrap_or(&link.link_text);
        g.add_link(path, target, graph::EdgeData {
            link_type: link.link_type.clone(),
            link_text: link.link_text.clone(),
            fragment: link.fragment.clone(),
        });
    }
}
```

- [ ] **Step 4: Update delete_file to maintain graph**

In `delete_file`, after removing from index, add:

```rust
// Remove from graph.
{
    let mut g = self.graph.write().expect("graph lock poisoned");
    g.remove_note(path);
}
```

- [ ] **Step 5: Add public graph query methods**

Add a new section to `impl StorageEngine` (before the Tasks section):

```rust
// ── Graph queries ────────────────────────────────────────────────────────

/// Return all files that link to the file at `path`.
///
/// # Errors
///
/// Returns [`StorageError`] if the graph lock is poisoned.
pub fn backlinks(&self, path: &str) -> Result<Vec<graph::BacklinkResult>, StorageError> {
    let g = self.graph.read().expect("graph lock poisoned");
    Ok(g.backlinks(path))
}

/// Return all outgoing links from the file at `path`.
///
/// # Errors
///
/// Returns [`StorageError`] if the graph lock is poisoned.
pub fn outgoing_links(&self, path: &str) -> Result<Vec<graph::OutgoingLink>, StorageError> {
    let g = self.graph.read().expect("graph lock poisoned");
    Ok(g.outgoing_links(path))
}

/// Return all unresolved (broken) links in the forge.
///
/// # Errors
///
/// Returns [`StorageError`] if the graph lock is poisoned.
pub fn unresolved_links(&self) -> Result<Vec<graph::UnresolvedLink>, StorageError> {
    let g = self.graph.read().expect("graph lock poisoned");
    Ok(g.unresolved_links())
}

/// Return knowledge graph statistics.
///
/// # Errors
///
/// Returns [`StorageError`] if the graph lock is poisoned.
pub fn graph_stats(&self) -> Result<graph::GraphStats, StorageError> {
    let g = self.graph.read().expect("graph lock poisoned");
    Ok(g.stats())
}

/// Return all files within `depth` hops of `path` in the knowledge graph.
///
/// # Errors
///
/// Returns [`StorageError`] if the graph lock is poisoned.
pub fn graph_neighbors(&self, path: &str, depth: usize) -> Result<Vec<String>, StorageError> {
    let g = self.graph.read().expect("graph lock poisoned");
    Ok(g.neighbors(path, depth))
}
```

- [ ] **Step 6: Verify compilation and existing tests**

Run: `cargo test -p nexus-storage`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add crates/nexus-storage/src/lib.rs
git commit -m "feat(storage): integrate KnowledgeGraph into StorageEngine with incremental updates"
```

---

### Task 7: CLI — Graph Commands and Content Links/Backlinks

**Files:**
- Modify: `crates/nexus-cli/src/main.rs`
- Create: `crates/nexus-cli/src/commands/graph.rs`
- Modify: `crates/nexus-cli/src/commands/content.rs`
- Modify: `crates/nexus-cli/src/commands/mod.rs`

- [ ] **Step 1: Create graph command handlers**

Create `crates/nexus-cli/src/commands/graph.rs`:

```rust
use anyhow::Result;

use crate::app::App;
use crate::output::{print_list, OutputFormat};

/// Show knowledge graph statistics.
pub fn status(app: &mut App) -> Result<()> {
    let storage = app.storage()?;
    let stats = storage
        .graph_stats()
        .map_err(|e| anyhow::anyhow!("failed to get graph stats: {e}"))?;

    let format = app.format();

    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            println!(
                "{}",
                serde_json::json!({
                    "nodes": stats.node_count,
                    "edges": stats.edge_count,
                    "unresolved": stats.unresolved_count,
                })
            );
        }
        _ => {
            println!("Nodes      : {}", stats.node_count);
            println!("Edges      : {}", stats.edge_count);
            println!("Unresolved : {}", stats.unresolved_count);
        }
    }

    Ok(())
}

/// List all unresolved (broken) links.
pub fn unresolved(app: &mut App) -> Result<()> {
    let storage = app.storage()?;
    let links = storage
        .unresolved_links()
        .map_err(|e| anyhow::anyhow!("failed to get unresolved links: {e}"))?;

    let format = app.format();

    if links.is_empty() {
        println!("No unresolved links.");
        return Ok(());
    }

    let headers = &["Target", "Referenced By"];
    let rows: Vec<Vec<String>> = links
        .iter()
        .map(|u| {
            vec![
                u.target_path.clone(),
                u.referenced_by.join(", "),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Show neighbors of a file within N hops.
pub fn neighbors(app: &mut App, path: &str, depth: usize) -> Result<()> {
    let storage = app.storage()?;
    let paths = storage
        .graph_neighbors(path, depth)
        .map_err(|e| anyhow::anyhow!("failed to get neighbors: {e}"))?;

    let format = app.format();

    if paths.is_empty() {
        println!("No neighbors found.");
        return Ok(());
    }

    let headers = &["Path"];
    let rows: Vec<Vec<String>> = paths.iter().map(|p| vec![p.clone()]).collect();

    print_list(format, headers, &rows);

    Ok(())
}
```

- [ ] **Step 2: Register graph module**

In `crates/nexus-cli/src/commands/mod.rs`, add:

```rust
pub mod graph;
```

- [ ] **Step 3: Add content links and backlinks handlers**

In `crates/nexus-cli/src/commands/content.rs`, add:

```rust
/// Show outgoing links from a file.
pub fn links(app: &mut App, path: &str) -> Result<()> {
    let storage = app.storage()?;
    let outgoing = storage
        .outgoing_links(path)
        .map_err(|e| anyhow::anyhow!("failed to get links: {e}"))?;

    let format = app.format();

    if outgoing.is_empty() {
        println!("No outgoing links.");
        return Ok(());
    }

    let headers = &["Target", "Type", "Text", "Resolved", "Fragment"];
    let rows: Vec<Vec<String>> = outgoing
        .iter()
        .map(|l| {
            vec![
                l.target_path.clone(),
                l.link_type.clone(),
                l.link_text.clone(),
                if l.is_resolved { "yes".to_string() } else { "no".to_string() },
                l.fragment.clone().unwrap_or_default(),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}

/// Show all files that link to the given file.
pub fn backlinks(app: &mut App, path: &str) -> Result<()> {
    let storage = app.storage()?;
    let bl = storage
        .backlinks(path)
        .map_err(|e| anyhow::anyhow!("failed to get backlinks: {e}"))?;

    let format = app.format();

    if bl.is_empty() {
        println!("No backlinks found.");
        return Ok(());
    }

    let headers = &["Source", "Type", "Text"];
    let rows: Vec<Vec<String>> = bl
        .iter()
        .map(|b| {
            vec![
                b.source_path.clone(),
                b.link_type.clone(),
                b.link_text.clone(),
            ]
        })
        .collect();

    print_list(format, headers, &rows);

    Ok(())
}
```

- [ ] **Step 4: Add CLI arg definitions and dispatch**

In `crates/nexus-cli/src/main.rs`, add the `Graph` command group. Replace the stub `Db(StubArgs)` line or add alongside it:

Add new arg structs after the existing `ContentCommand` enum:

```rust
// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct GraphArgs {
    #[command(subcommand)]
    command: GraphCommand,
}

#[derive(Subcommand)]
enum GraphCommand {
    /// Show knowledge graph statistics
    Status,
    /// List unresolved (broken) links
    Unresolved,
    /// Show files within N hops of a file
    Neighbors {
        /// Path of the file
        path: String,
        /// Maximum traversal depth
        #[arg(short, long, default_value_t = 1)]
        depth: usize,
    },
}
```

Add two new variants to `ContentCommand`:

```rust
/// Show outgoing links from a file
Links {
    /// Path of the file
    path: String,
},
/// Show files that link to this file
Backlinks {
    /// Path of the file
    path: String,
},
```

Replace the `Db(StubArgs)` line in `Commands` with:

```rust
/// Knowledge graph operations
Graph(GraphArgs),
```

Add dispatch in `main()`:

```rust
Commands::Graph(args) => match args.command {
    GraphCommand::Status => commands::graph::status(&mut app),
    GraphCommand::Unresolved => commands::graph::unresolved(&mut app),
    GraphCommand::Neighbors { path, depth } => {
        commands::graph::neighbors(&mut app, &path, depth)
    }
},
```

And in the `Commands::Content` match:

```rust
ContentCommand::Links { path } => commands::content::links(&mut app, &path),
ContentCommand::Backlinks { path } => commands::content::backlinks(&mut app, &path),
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/nexus-cli/src/commands/graph.rs crates/nexus-cli/src/commands/mod.rs crates/nexus-cli/src/commands/content.rs crates/nexus-cli/src/main.rs
git commit -m "feat(cli): add graph status/unresolved/neighbors and content links/backlinks commands"
```

---

### Task 8: Integration Tests

**Files:**
- Create: `crates/nexus-storage/tests/prd-06-phase1-smoke.rs`

- [ ] **Step 1: Write integration tests**

Create `crates/nexus-storage/tests/prd-06-phase1-smoke.rs`:

```rust
//! Growth Plan Phase 1 smoke tests — knowledge graph + backlinks.

use nexus_storage::{FileFilter, StorageEngine};

fn engine() -> (tempfile::TempDir, StorageEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(dir.path()).unwrap();
    (dir, engine)
}

#[test]
fn backlinks_and_outgoing_links() {
    let (_dir, engine) = engine();

    engine.write_file("notes/a.md", b"# A\n\nLink to [[notes/b.md]]\n").unwrap();
    engine.write_file("notes/b.md", b"# B\n\nLink to [[notes/a.md]]\n").unwrap();
    engine.write_file("notes/c.md", b"# C\n\nLink to [[notes/b.md]]\n").unwrap();

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

    engine.write_file("notes/a.md", b"See [[notes/missing.md]]\n").unwrap();

    let unresolved = engine.unresolved_links().unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].target_path, "notes/missing.md");
    assert!(unresolved[0].referenced_by.contains(&"notes/a.md".to_string()));
}

#[test]
fn adding_file_resolves_phantom() {
    let (_dir, engine) = engine();

    engine.write_file("notes/a.md", b"See [[notes/b.md]]\n").unwrap();
    assert_eq!(engine.unresolved_links().unwrap().len(), 1);

    // Creating b.md should resolve the phantom
    engine.write_file("notes/b.md", b"# B\n").unwrap();
    assert_eq!(engine.unresolved_links().unwrap().len(), 0);

    // And backlinks should now work
    let bl = engine.backlinks("notes/b.md").unwrap();
    assert_eq!(bl.len(), 1);
}

#[test]
fn deleting_file_updates_graph() {
    let (_dir, engine) = engine();

    engine.write_file("notes/a.md", b"Link to [[notes/b.md]]\n").unwrap();
    engine.write_file("notes/b.md", b"# B\n").unwrap();

    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 1);

    engine.delete_file("notes/a.md").unwrap();

    // Backlinks to b should be gone
    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 0);
}

#[test]
fn graph_stats_correct() {
    let (_dir, engine) = engine();

    engine.write_file("notes/a.md", b"[[notes/b.md]] and [[notes/c.md]]\n").unwrap();
    engine.write_file("notes/b.md", b"# B\n").unwrap();

    let stats = engine.graph_stats().unwrap();
    assert_eq!(stats.node_count, 3); // a, b, c(phantom)
    assert_eq!(stats.edge_count, 2);
    assert_eq!(stats.unresolved_count, 1); // c is phantom
}

#[test]
fn graph_neighbors_traversal() {
    let (_dir, engine) = engine();

    engine.write_file("notes/a.md", b"[[notes/b.md]]\n").unwrap();
    engine.write_file("notes/b.md", b"[[notes/c.md]]\n").unwrap();
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

    engine.write_file("notes/a.md", b"[[notes/b.md]]\n").unwrap();
    engine.write_file("notes/b.md", b"# B\n").unwrap();

    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 1);

    // Rewrite a.md without the link to b
    engine.write_file("notes/a.md", b"# A\n\nNo links here.\n").unwrap();

    assert_eq!(engine.backlinks("notes/b.md").unwrap().len(), 0);
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test -p nexus-storage --test prd-06-phase1-smoke`
Expected: All PASS

- [ ] **Step 3: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All PASS (except known flaky credential vault test)

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace`
Expected: No new warnings

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-storage/tests/prd-06-phase1-smoke.rs
git commit -m "test(storage): add knowledge graph integration tests — backlinks, unresolved, neighbors, rewrite"
```
