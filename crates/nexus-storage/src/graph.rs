//! In-memory knowledge graph built on petgraph.
//!
//! Represents notes as nodes and links as directed edges.
//! Rebuilt from `SQLite` on startup, updated incrementally on file changes.

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};

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

/// A file that links TO the queried path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacklinkResult {
    /// Path of the file containing the link.
    pub source_path: String,
    /// Display text of the link.
    pub link_text: String,
    /// Kind of link.
    pub link_type: String,
}

/// A link FROM the queried path to another file.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedLink {
    /// The missing target path.
    pub target_path: String,
    /// Paths of files that reference this target.
    pub referenced_by: Vec<String>,
}

/// One node entry in the bulk graph response. Phantom (unresolved) nodes
/// are included so the frontend can render dimmed link targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNodeEntry {
    /// Forge-relative path of the file (or unresolved link target).
    pub path: String,
    /// True when this node is a phantom (no real file backs it).
    pub is_phantom: bool,
}

/// One directed edge in the bulk graph response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdgeEntry {
    /// Source file path.
    pub source: String,
    /// Target file path (may be a phantom).
    pub target: String,
    /// True when the target file actually exists in the forge.
    pub is_resolved: bool,
}

/// Bulk projection of the entire link graph for a single IPC call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    /// All nodes (real + phantom).
    pub nodes: Vec<GraphNodeEntry>,
    /// All directed edges.
    pub edges: Vec<GraphEdgeEntry>,
}

/// Summary statistics for the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    /// Total number of nodes (files + phantoms).
    pub node_count: usize,
    /// Total number of directed edges.
    pub edge_count: usize,
    /// Number of phantom (unresolved) nodes.
    pub unresolved_count: usize,
}

/// In-memory directed graph of notes and links.
pub struct KnowledgeGraph {
    graph: StableGraph<NodeData, EdgeData>,
    path_to_node: HashMap<String, NodeIndex>,
    phantom_nodes: HashSet<NodeIndex>,
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeGraph {
    /// Create an empty knowledge graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            path_to_node: HashMap::new(),
            phantom_nodes: HashSet::new(),
        }
    }

    /// Add a note to the graph. Idempotent -- returns existing index if present.
    /// If the path was a phantom node, promotes it to real.
    pub fn add_note(&mut self, path: &str) -> NodeIndex {
        if let Some(&idx) = self.path_to_node.get(path) {
            self.phantom_nodes.remove(&idx);
            return idx;
        }
        let idx = self.graph.add_node(NodeData {
            path: path.to_string(),
        });
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
            let idx = self.graph.add_node(NodeData {
                path: target.to_string(),
            });
            self.path_to_node.insert(target.to_string(), idx);
            self.phantom_nodes.insert(idx);
            idx
        };
        self.graph.add_edge(src_idx, tgt_idx, edge);
    }

    /// Remove all outgoing edges from `source`, keeping the node.
    pub fn remove_links_from(&mut self, source: &str) {
        if let Some(&idx) = self.path_to_node.get(source) {
            let outgoing: Vec<_> = self
                .graph
                .edges_directed(idx, Direction::Outgoing)
                .map(|e| e.id())
                .collect();
            for edge_id in outgoing {
                self.graph.remove_edge(edge_id);
            }
        }
    }

    /// Return graph statistics.
    #[must_use] 
    pub fn stats(&self) -> GraphStats {
        GraphStats {
            node_count: self.graph.node_count(),
            edge_count: self.graph.edge_count(),
            unresolved_count: self.phantom_nodes.len(),
        }
    }

    /// Return all files that link TO `path`.
    #[must_use] 
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
    #[must_use] 
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
    #[must_use] 
    pub fn unresolved_links(&self) -> Vec<UnresolvedLink> {
        self.phantom_nodes
            .iter()
            .map(|&idx| {
                let target_path = self.graph[idx].path.clone();
                let referenced_by: Vec<String> = self
                    .graph
                    .edges_directed(idx, Direction::Incoming)
                    .map(|e| self.graph[e.source()].path.clone())
                    .collect();
                UnresolvedLink {
                    target_path,
                    referenced_by,
                }
            })
            .collect()
    }

    /// Return a flat projection of every node and every edge in the graph.
    ///
    /// Used by the bulk `list_all_links` IPC handler to power the global
    /// graph view. Edges are emitted once per stored edge (no dedup); the
    /// caller can collapse duplicates if it wants.
    #[must_use]
    pub fn snapshot(&self) -> GraphSnapshot {
        let mut nodes = Vec::with_capacity(self.graph.node_count());
        for idx in self.graph.node_indices() {
            nodes.push(GraphNodeEntry {
                path: self.graph[idx].path.clone(),
                is_phantom: self.phantom_nodes.contains(&idx),
            });
        }

        let mut edges = Vec::with_capacity(self.graph.edge_count());
        for src in self.graph.node_indices() {
            for edge in self.graph.edges_directed(src, Direction::Outgoing) {
                let tgt = edge.target();
                edges.push(GraphEdgeEntry {
                    source: self.graph[src].path.clone(),
                    target: self.graph[tgt].path.clone(),
                    is_resolved: !self.phantom_nodes.contains(&tgt),
                });
            }
        }

        GraphSnapshot { nodes, edges }
    }

    /// BFS traversal from `path` up to `depth` hops in both directions.
    /// Returns unique paths excluding the start node.
    #[must_use] 
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

    /// Build the knowledge graph from the `SQLite` index.
    ///
    /// Queries all non-deleted files as nodes and all links as edges.
    /// Unresolved link targets become phantom nodes.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn rebuild_from_db(conn: &rusqlite::Connection) -> Result<Self, crate::StorageError> {
        let mut kg = Self::new();

        // 1. Add all files as nodes
        let mut stmt = conn.prepare("SELECT path FROM files WHERE is_deleted = 0;")?;
        let paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(std::result::Result::ok)
            .collect();
        for path in &paths {
            kg.add_note(path);
        }

        // 2. Add all links as edges
        let mut stmt = conn.prepare(
            "SELECT f.path, l.target_path, l.link_text, l.link_type, l.fragment
             FROM links l
             JOIN files f ON f.id = l.source_file_id
             WHERE f.is_deleted = 0 AND l.target_path IS NOT NULL;",
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
            .filter_map(std::result::Result::ok)
            .collect();

        for (source, target, link_text, link_type, fragment) in links {
            kg.add_link(
                &source,
                &target,
                EdgeData {
                    link_type,
                    link_text,
                    fragment,
                },
            );
        }

        Ok(kg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_stats() {
        let kg = KnowledgeGraph::new();
        let s = kg.stats();
        assert_eq!(s.node_count, 0);
        assert_eq!(s.edge_count, 0);
        assert_eq!(s.unresolved_count, 0);
    }

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
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
        assert_eq!(kg.stats().edge_count, 1);
    }

    #[test]
    fn add_link_creates_phantom_for_missing_target() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_link(
            "notes/a.md",
            "notes/missing.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "missing".to_string(),
                fragment: None,
            },
        );
        assert_eq!(kg.stats().node_count, 2);
        assert_eq!(kg.stats().unresolved_count, 1);
    }

    #[test]
    fn add_note_promotes_phantom() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
        assert_eq!(kg.stats().unresolved_count, 1);
        kg.add_note("notes/b.md");
        assert_eq!(kg.stats().unresolved_count, 0);
        assert_eq!(kg.stats().node_count, 2);
    }

    #[test]
    fn remove_note_removes_edges() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_note("notes/b.md");
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
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
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
        kg.add_link(
            "notes/a.md",
            "notes/c.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "c".to_string(),
                fragment: None,
            },
        );
        assert_eq!(kg.stats().edge_count, 2);
        kg.remove_links_from("notes/a.md");
        assert_eq!(kg.stats().edge_count, 0);
        assert_eq!(kg.stats().node_count, 3);
    }

    // ── Task 4: Query method tests ───────────────────────────────────────────

    #[test]
    fn backlinks_returns_incoming() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_note("notes/b.md");
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
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
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
        kg.add_link(
            "notes/a.md",
            "notes/missing.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "missing".to_string(),
                fragment: Some("heading".to_string()),
            },
        );
        let out = kg.outgoing_links("notes/a.md");
        assert_eq!(out.len(), 2);
        let resolved = out.iter().find(|l| l.target_path == "notes/b.md").unwrap();
        assert!(resolved.is_resolved);
        let unresolved = out
            .iter()
            .find(|l| l.target_path == "notes/missing.md")
            .unwrap();
        assert!(!unresolved.is_resolved);
        assert_eq!(unresolved.fragment, Some("heading".to_string()));
    }

    #[test]
    fn unresolved_links_lists_phantoms() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_note("notes/b.md");
        kg.add_link(
            "notes/a.md",
            "notes/missing1.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "m1".to_string(),
                fragment: None,
            },
        );
        kg.add_link(
            "notes/b.md",
            "notes/missing1.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "m1".to_string(),
                fragment: None,
            },
        );
        kg.add_link(
            "notes/a.md",
            "notes/missing2.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "m2".to_string(),
                fragment: None,
            },
        );
        let unresolved = kg.unresolved_links();
        assert_eq!(unresolved.len(), 2);
        let m1 = unresolved
            .iter()
            .find(|u| u.target_path == "notes/missing1.md")
            .unwrap();
        assert_eq!(m1.referenced_by.len(), 2);
    }

    #[test]
    fn neighbors_bfs_depth_1() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_note("notes/b.md");
        kg.add_note("notes/c.md");
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
        kg.add_link(
            "notes/b.md",
            "notes/c.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "c".to_string(),
                fragment: None,
            },
        );
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
        kg.add_link(
            "notes/a.md",
            "notes/b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
        kg.add_link(
            "notes/b.md",
            "notes/c.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "c".to_string(),
                fragment: None,
            },
        );
        let n = kg.neighbors("notes/a.md", 2);
        assert_eq!(n.len(), 2);
        assert!(n.contains(&"notes/b.md".to_string()));
        assert!(n.contains(&"notes/c.md".to_string()));
    }

    // ── Task 5: rebuild_from_db test ─────────────────────────────────────────

    #[test]
    fn rebuild_from_db() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('notes/a.md', 'markdown', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let a_id = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('notes/b.md', 'markdown', 'h2', 10, 0, 0);",
            [],
        )
        .unwrap();
        let b_id = conn.last_insert_rowid();

        // a links to b (resolved)
        conn.execute(
            "INSERT INTO links (source_file_id, target_path, target_file_id, link_text, link_type, is_resolved)
             VALUES (?1, 'notes/b.md', ?2, 'b', 'wikilink', 1);",
            rusqlite::params![a_id, b_id],
        )
        .unwrap();

        // a links to missing (unresolved)
        conn.execute(
            "INSERT INTO links (source_file_id, target_path, link_text, link_type, is_resolved)
             VALUES (?1, 'notes/missing.md', 'missing', 'wikilink', 0);",
            rusqlite::params![a_id],
        )
        .unwrap();

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

    // ── snapshot() tests ────────────────────────────────────────────────────

    #[test]
    fn snapshot_empty_graph() {
        let snap = KnowledgeGraph::new().snapshot();
        assert!(snap.nodes.is_empty());
        assert!(snap.edges.is_empty());
    }

    #[test]
    fn snapshot_single_note_no_links() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        let snap = kg.snapshot();
        assert_eq!(snap.nodes.len(), 1);
        assert!(snap.edges.is_empty());
        assert_eq!(snap.nodes[0].path, "notes/a.md");
        assert!(!snap.nodes[0].is_phantom);
    }

    #[test]
    fn snapshot_unresolved_link_keeps_phantom_node() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("notes/a.md");
        kg.add_link(
            "notes/a.md",
            "notes/missing.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "missing".to_string(),
                fragment: None,
            },
        );
        let snap = kg.snapshot();
        assert_eq!(snap.nodes.len(), 2);
        let phantom = snap.nodes.iter().find(|n| n.path == "notes/missing.md").unwrap();
        assert!(phantom.is_phantom);
        assert_eq!(snap.edges.len(), 1);
        assert!(!snap.edges[0].is_resolved);
    }

    #[test]
    fn snapshot_cycle_two_notes() {
        let mut kg = KnowledgeGraph::new();
        kg.add_note("a.md");
        kg.add_note("b.md");
        kg.add_link(
            "a.md",
            "b.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "b".to_string(),
                fragment: None,
            },
        );
        kg.add_link(
            "b.md",
            "a.md",
            EdgeData {
                link_type: "wikilink".to_string(),
                link_text: "a".to_string(),
                fragment: None,
            },
        );
        let snap = kg.snapshot();
        assert_eq!(snap.nodes.len(), 2);
        assert_eq!(snap.edges.len(), 2);
        assert!(snap.edges.iter().all(|e| e.is_resolved));
    }

    #[test]
    fn snapshot_dense_subgraph() {
        let mut kg = KnowledgeGraph::new();
        let paths = ["a.md", "b.md", "c.md", "d.md"];
        for p in &paths {
            kg.add_note(p);
        }
        for s in &paths {
            for t in &paths {
                if s == t {
                    continue;
                }
                kg.add_link(
                    s,
                    t,
                    EdgeData {
                        link_type: "wikilink".to_string(),
                        link_text: (*t).to_string(),
                        fragment: None,
                    },
                );
            }
        }
        let snap = kg.snapshot();
        assert_eq!(snap.nodes.len(), 4);
        assert_eq!(snap.edges.len(), 12);
    }
}
