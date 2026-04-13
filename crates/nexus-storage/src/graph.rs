//! In-memory knowledge graph built on petgraph.
//!
//! Represents notes as nodes and links as directed edges.
//! Rebuilt from SQLite on startup, updated incrementally on file changes.

#[allow(unused_imports)]
use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

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
    pub fn stats(&self) -> GraphStats {
        GraphStats {
            node_count: self.graph.node_count(),
            edge_count: self.graph.edge_count(),
            unresolved_count: self.phantom_nodes.len(),
        }
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
}
