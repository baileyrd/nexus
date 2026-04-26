# Knowledge Graph + Backlinks Design

**Date:** 2026-04-13
**Status:** Approved
**Scope:** In-memory petgraph knowledge graph, backlink queries, CLI commands
**Source:** Growth Plan Phase 1, Backlog BL-005

---

## Overview

Build a live in-memory knowledge graph using `petgraph` that represents all notes and links in the forge. The graph is built from the SQLite `links` table on startup and updated incrementally on every file write/delete. Exposes backlink queries, unresolved link detection, and BFS neighbor traversal through the StorageEngine API and CLI.

TUI backlinks panel is deferred to a separate pass.

---

## 1. KnowledgeGraph Core (`crates/nexus-storage/src/graph.rs`)

New file containing the `KnowledgeGraph` struct.

### 1.1 Data Structures

```rust
struct NodeData { path: String }
struct EdgeData { link_type: String, link_text: String, fragment: Option<String> }
```

Internal state:
- `graph: StableGraph<NodeData, EdgeData, Directed>` — the petgraph graph
- `path_to_node: HashMap<String, NodeIndex>` — O(1) path lookup
- `phantom_nodes: HashSet<NodeIndex>` — nodes with no corresponding file (unresolved link targets)

A node is "phantom" if it was created as a link target but no file with that path exists in the forge. When a file is added whose path matches a phantom node, the node is promoted to real (removed from `phantom_nodes`).

### 1.2 Public API

**Construction:**
- `new() -> Self` — empty graph
- `rebuild_from_db(conn: &Connection) -> Result<Self, StorageError>` — query all non-deleted files as nodes, all links as edges. Targets with no matching file become phantom nodes.

**Mutation:**
- `add_note(&mut self, path: &str) -> NodeIndex` — idempotent. If path matches a phantom node, promotes it to real.
- `remove_note(&mut self, path: &str)` — removes node and all incident edges. No-op if path not in graph.
- `add_link(&mut self, source: &str, target: &str, edge: EdgeData)` — creates directed edge. Auto-creates phantom target node if target path isn't in the graph.
- `remove_links_from(&mut self, source: &str)` — removes all outgoing edges from a node. Used before re-adding links on file re-parse. Does not remove the node itself.

**Queries:**
- `backlinks(&self, path: &str) -> Vec<BacklinkResult>` — all edges pointing TO this node, returning source path + link metadata.
- `outgoing_links(&self, path: &str) -> Vec<OutgoingLink>` — all edges FROM this node, with `is_resolved` flag based on whether target is phantom.
- `unresolved_links(&self) -> Vec<UnresolvedLink>` — all phantom nodes, each with the list of source paths that reference them.
- `neighbors(&self, path: &str, depth: usize) -> Vec<String>` — BFS traversal up to N hops (both directions), returning unique paths excluding the start node.
- `stats(&self) -> GraphStats` — node count, edge count, unresolved (phantom) count.

### 1.3 Return Types

```rust
pub struct BacklinkResult {
    pub source_path: String,
    pub link_text: String,
    pub link_type: String,
}

pub struct OutgoingLink {
    pub target_path: String,
    pub link_text: String,
    pub link_type: String,
    pub is_resolved: bool,
    pub fragment: Option<String>,
}

pub struct UnresolvedLink {
    pub target_path: String,
    pub referenced_by: Vec<String>,
}

pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub unresolved_count: usize,
}
```

---

## 2. StorageEngine Integration

### 2.1 New Field

Add `graph: Arc<RwLock<KnowledgeGraph>>` to `StorageEngine`.

### 2.2 Initialization

In `open_internal`, after reconcile completes:

```rust
let graph = KnowledgeGraph::rebuild_from_db(&write_conn)?;
let graph = Arc::new(RwLock::new(graph));
```

For `init` (new forge), the graph starts empty since there are no files yet.

### 2.3 Write Path

In `write_file`, after the existing index insert:

1. Acquire write lock on graph
2. `graph.add_note(path)` — ensure node exists (promotes phantom if applicable)
3. `graph.remove_links_from(path)` — clear stale outgoing edges
4. For each link in `parsed.links`, call `graph.add_link(path, target, edge_data)` using the link's `target_path` (or `link_text` for bare wikilinks), `link_type`, `link_text`, and `fragment`

### 2.4 Delete Path

In `delete_file`, after removing from index:

1. Acquire write lock on graph
2. `graph.remove_note(path)` — removes node and all incident edges

### 2.5 Public Methods on StorageEngine

```rust
pub fn backlinks(&self, path: &str) -> Result<Vec<BacklinkResult>, StorageError>
pub fn outgoing_links(&self, path: &str) -> Result<Vec<OutgoingLink>, StorageError>
pub fn unresolved_links(&self) -> Result<Vec<UnresolvedLink>, StorageError>
pub fn graph_stats(&self) -> Result<GraphStats, StorageError>
pub fn graph_neighbors(&self, path: &str, depth: usize) -> Result<Vec<String>, StorageError>
```

Each acquires a read lock on the graph, calls the corresponding method, and returns. The write lock is only held during `write_file` and `delete_file` graph mutations — the same code paths that already hold the `write_conn` mutex, so no new contention patterns.

---

## 3. CLI Commands

### 3.1 Content Subcommands (existing group)

Two new variants on `ContentCommand`:

```
nexus content links <path>       — show outgoing links from a file
nexus content backlinks <path>   — show all files linking to this file
```

Text output format:
```
Source              Type       Text            Fragment
notes/other.md     wikilink   "other note"
notes/ref.md       embed      "ref"           #heading
```

### 3.2 New Graph Command Group

Replace the existing `Graph` stub (if present) or add as a new top-level command:

```
nexus graph status                     — node count, edge count, unresolved count
nexus graph unresolved                 — list all broken links with referencing files
nexus graph neighbors <path> [-d 2]    — files within N hops (default depth=1)
```

All commands respect `--format` flag (text/json/table).

### 3.3 Implementation

- `GraphArgs` / `GraphCommand` enum in `main.rs`
- Handler functions in a new `commands/graph.rs` module
- `content links` and `content backlinks` handlers in `commands/content.rs`

---

## 4. Testing Strategy

### 4.1 Unit Tests (`graph.rs`)

- Empty graph stats are zero
- `add_note` is idempotent
- `add_link` creates edge, `backlinks` returns it
- `add_link` to non-existent target creates phantom node
- `remove_note` removes node and all edges
- `remove_links_from` clears outgoing edges, preserves node
- `unresolved_links` returns phantom nodes with their referrers
- `add_note` for a phantom target promotes it to real
- `neighbors` BFS at depth 1 and depth 2
- `rebuild_from_db` with in-memory SQLite

### 4.2 Integration Tests (`tests/prd-06-phase1-smoke.rs`)

- Write 3 files with cross-links, verify backlinks and outgoing_links via StorageEngine
- Write file that resolves a previously unresolved link, verify unresolved_links shrinks
- Delete a file, verify backlinks disappear and links to it become unresolved
- `graph_stats` returns correct counts after writes/deletes
- `graph_neighbors` returns correct nodes at depth 1 and 2

---

## 5. Dependencies

New workspace dependency:
```toml
petgraph = "0.7"
```

Added to `crates/nexus-storage/Cargo.toml`.

No other new dependencies. `Arc` and `RwLock` are from `std::sync`.

---

## 6. Files Changed

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add `petgraph = "0.7"` |
| `crates/nexus-storage/Cargo.toml` | Add `petgraph = { workspace = true }` |
| `crates/nexus-storage/src/graph.rs` | **NEW** — KnowledgeGraph, all types and methods |
| `crates/nexus-storage/src/lib.rs` | Add graph field to StorageEngine, expose methods, update write/delete paths |
| `crates/nexus-storage/src/index.rs` | No changes (graph uses existing link data) |
| `crates/nexus-cli/src/main.rs` | Add Graph command group, Links/Backlinks content subcommands |
| `crates/nexus-cli/src/commands/graph.rs` | **NEW** — graph command handlers |
| `crates/nexus-cli/src/commands/content.rs` | Add links/backlinks handlers |
| `crates/nexus-cli/src/commands/mod.rs` | Register graph module |
| `crates/nexus-storage/tests/prd-06-phase1-smoke.rs` | **NEW** — integration tests |

---

## Out of Scope

- TUI backlinks panel (deferred)
- Event publishing via kernel EventBus (deferred to when EventBus is wired to storage)
- Graph visualization (ASCII art or otherwise)
- Graph persistence to disk (rebuilt from SQLite on every open)
