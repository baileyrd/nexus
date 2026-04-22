//! Canvas file format parser, serializer, and `SQLite` persistence.
//!
//! Format types (`CanvasFile`, `CanvasNode`, etc.) and pure parsing helpers
//! live in `nexus-formats`; this module re-exports them and adds `SQLite`
//! persistence on top.

use rusqlite::Connection;

use crate::StorageError;

// ── Format types (owned by nexus-formats) ────────────────────────────────────

pub use nexus_formats::canvas::{
    CanvasEdge, CanvasEdgeType, CanvasFile, CanvasNode, CanvasNodeType,
};

/// A canvas node record from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasNodeRecord {
    /// Database row ID.
    pub id: i64,
    /// Owning file ID.
    pub file_id: i64,
    /// Node identifier within the canvas.
    pub node_id: String,
    /// Node type.
    pub node_type: String,
    /// Horizontal position.
    pub x: f64,
    /// Vertical position.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
    /// Display color.
    pub color: Option<String>,
    /// Display label.
    pub label: Option<String>,
    /// Collapsed state.
    pub collapsed: bool,
    /// Type-specific content as JSON.
    pub content_json: Option<String>,
}

/// A canvas edge record from the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasEdgeRecord {
    /// Database row ID.
    pub id: i64,
    /// Owning file ID.
    pub file_id: i64,
    /// Edge identifier.
    pub edge_id: String,
    /// Source node ID.
    pub from_node: String,
    /// Target node ID.
    pub to_node: String,
    /// Edge line style.
    pub edge_type: String,
    /// Relationship label.
    pub label: Option<String>,
    /// Display color.
    pub color: Option<String>,
}

// ── Parser / Serializer ──────────────────────────────────────────────────────

/// Parse a `.canvas` JSON string into a [`CanvasFile`].
///
/// Delegates to [`nexus_formats::canvas::parse`]; maps
/// [`nexus_formats::error::CanvasError`] to [`StorageError::CorruptFile`].
///
/// # Errors
///
/// Returns [`StorageError::CorruptFile`] if the JSON is invalid.
pub fn parse_canvas(json: &str) -> Result<CanvasFile, StorageError> {
    nexus_formats::canvas::parse(json).map_err(|e| StorageError::CorruptFile {
        path: "<canvas>".to_string(),
        reason: e.to_string(),
    })
}

/// Serialize a [`CanvasFile`] to pretty-printed JSON.
///
/// Delegates to [`nexus_formats::canvas::serialize`].
///
/// # Errors
///
/// Returns [`StorageError::CorruptFile`] on serialization failure.
pub fn serialize_canvas(canvas: &CanvasFile) -> Result<String, StorageError> {
    nexus_formats::canvas::serialize(canvas).map_err(|e| StorageError::CorruptFile {
        path: "<canvas>".to_string(),
        reason: e.to_string(),
    })
}

// ── DB Operations ────────────────────────────────────────────────────────────

/// Persist canvas nodes and edges to `SQLite`.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn insert_canvas(
    conn: &Connection,
    file_id: i64,
    canvas: &CanvasFile,
) -> Result<(), StorageError> {
    let mut node_stmt = conn.prepare_cached(
        "INSERT INTO canvas_nodes (file_id, node_id, node_type, x, y, width, height, color, label, collapsed, content_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11);",
    )?;
    for node in &canvas.nodes {
        let content_json = node_content_json(node);
        node_stmt.execute(rusqlite::params![
            file_id,
            node.id,
            node.node_type.as_str(),
            node.x,
            node.y,
            node.width,
            node.height,
            node.color,
            node.label,
            node.collapsed,
            content_json,
        ])?;
    }

    let mut edge_stmt = conn.prepare_cached(
        "INSERT INTO canvas_edges (file_id, edge_id, from_node, to_node, edge_type, label, color)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
    )?;
    for edge in &canvas.edges {
        edge_stmt.execute(rusqlite::params![
            file_id,
            edge.id,
            edge.from_node,
            edge.to_node,
            edge.edge_type.as_str(),
            edge.label,
            edge.color,
        ])?;
    }

    Ok(())
}

/// Query all canvas nodes for a given file.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_canvas_nodes(
    conn: &Connection,
    file_id: i64,
) -> Result<Vec<CanvasNodeRecord>, StorageError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, file_id, node_id, node_type, x, y, width, height, color, label, collapsed, content_json
         FROM canvas_nodes WHERE file_id = ?1 ORDER BY id;",
    )?;
    let rows = stmt.query_map(rusqlite::params![file_id], |row| {
        Ok(CanvasNodeRecord {
            id: row.get(0)?,
            file_id: row.get(1)?,
            node_id: row.get(2)?,
            node_type: row.get(3)?,
            x: row.get(4)?,
            y: row.get(5)?,
            width: row.get(6)?,
            height: row.get(7)?,
            color: row.get(8)?,
            label: row.get(9)?,
            collapsed: row.get(10)?,
            content_json: row.get(11)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Query all canvas edges for a given file.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_canvas_edges(
    conn: &Connection,
    file_id: i64,
) -> Result<Vec<CanvasEdgeRecord>, StorageError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, file_id, edge_id, from_node, to_node, edge_type, label, color
         FROM canvas_edges WHERE file_id = ?1 ORDER BY id;",
    )?;
    let rows = stmt.query_map(rusqlite::params![file_id], |row| {
        Ok(CanvasEdgeRecord {
            id: row.get(0)?,
            file_id: row.get(1)?,
            edge_id: row.get(2)?,
            from_node: row.get(3)?,
            to_node: row.get(4)?,
            edge_type: row.get(5)?,
            label: row.get(6)?,
            color: row.get(7)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Delete all canvas data for a given file.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn delete_canvas(conn: &Connection, file_id: i64) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM canvas_nodes WHERE file_id = ?1;",
        rusqlite::params![file_id],
    )?;
    conn.execute(
        "DELETE FROM canvas_edges WHERE file_id = ?1;",
        rusqlite::params![file_id],
    )?;
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract type-specific content from a node as JSON for storage.
fn node_content_json(node: &CanvasNode) -> Option<String> {
    let obj = match node.node_type {
        CanvasNodeType::File => node.file.as_ref().map(|f| serde_json::json!({"file": f})),
        CanvasNodeType::Text => node.text.as_ref().map(|t| serde_json::json!({"text": t})),
        CanvasNodeType::Link => node.url.as_ref().map(|u| serde_json::json!({"url": u})),
        CanvasNodeType::Group => node.label.as_ref().map(|l| serde_json::json!({"label": l})),
        CanvasNodeType::Database => node.source.as_ref().map(|s| serde_json::json!({"source": s})),
        CanvasNodeType::Terminal => node.command.as_ref().map(|c| serde_json::json!({"command": c})),
    };
    obj.map(|v| v.to_string())
}

/// Extract file paths referenced by file-type nodes.
#[must_use] 
pub fn extract_file_links(canvas: &CanvasFile) -> Vec<String> {
    canvas
        .nodes
        .iter()
        .filter_map(|n| {
            if n.node_type == CanvasNodeType::File {
                n.file.clone()
            } else {
                None
            }
        })
        .collect()
}

// ── Patch ops ────────────────────────────────────────────────────────────────

/// A single edit applied to a [`CanvasFile`] by [`apply_patch`].
///
/// The shell sends a list of these through the `canvas_patch` IPC handler —
/// the kernel re-reads the file, applies the list in order, then rewrites.
/// Keep the op set small: the goal is minimal round-trip cost for drag /
/// create / delete. Fine-grained edits (label, colour, style) use
/// [`Self::NodeUpdate`] / [`Self::EdgeUpdate`] which replace the whole
/// record in place by id.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CanvasPatchOp {
    /// Append a new node. Fails if the id already exists.
    NodeAdd {
        /// Node to insert.
        node: CanvasNode,
    },
    /// Remove a node by id. Also removes any incident edges. No-op if the
    /// id is unknown.
    NodeRemove {
        /// Target node id.
        id: String,
    },
    /// Move a node to new coordinates. No-op if the id is unknown.
    NodeMove {
        /// Target node id.
        id: String,
        /// New x coordinate.
        x: f64,
        /// New y coordinate.
        y: f64,
    },
    /// Replace a node by id with the supplied record. No-op if the id is
    /// unknown — callers wanting upsert semantics should use
    /// [`Self::NodeAdd`] followed by this op.
    NodeUpdate {
        /// Replacement node. `node.id` identifies the target.
        node: CanvasNode,
    },
    /// Append a new edge. Fails if the id already exists.
    EdgeAdd {
        /// Edge to insert.
        edge: CanvasEdge,
    },
    /// Remove an edge by id. No-op if the id is unknown.
    EdgeRemove {
        /// Target edge id.
        id: String,
    },
    /// Replace an edge by id with the supplied record.
    EdgeUpdate {
        /// Replacement edge. `edge.id` identifies the target.
        edge: CanvasEdge,
    },
}

/// Error cases [`apply_patch`] surfaces back to the caller.
///
/// Distinct from [`StorageError`] so the IPC layer can map duplicate-id
/// failures to a specific error rather than a generic corrupt-file.
#[derive(Debug, thiserror::Error)]
pub enum CanvasPatchError {
    /// An `NodeAdd` op tried to insert a node whose id is already present.
    #[error("canvas already has node id '{0}'")]
    DuplicateNodeId(String),
    /// An `EdgeAdd` op tried to insert an edge whose id is already present.
    #[error("canvas already has edge id '{0}'")]
    DuplicateEdgeId(String),
}

/// Apply each op in order to `canvas`, mutating it in place.
///
/// Semantics per op are documented on [`CanvasPatchOp`]. Unknown-id removes
/// / moves / updates are deliberate no-ops so that optimistic client-side
/// state that races a concurrent save doesn't crash the patch. Adds with a
/// duplicate id are rejected to prevent silent data loss.
///
/// # Errors
///
/// Returns [`CanvasPatchError`] on duplicate-id add.
pub fn apply_patch(
    canvas: &mut CanvasFile,
    ops: &[CanvasPatchOp],
) -> Result<(), CanvasPatchError> {
    for op in ops {
        match op {
            CanvasPatchOp::NodeAdd { node } => {
                if canvas.nodes.iter().any(|n| n.id == node.id) {
                    return Err(CanvasPatchError::DuplicateNodeId(node.id.clone()));
                }
                canvas.nodes.push(node.clone());
            }
            CanvasPatchOp::NodeRemove { id } => {
                canvas.nodes.retain(|n| &n.id != id);
                canvas
                    .edges
                    .retain(|e| &e.from_node != id && &e.to_node != id);
            }
            CanvasPatchOp::NodeMove { id, x, y } => {
                if let Some(n) = canvas.nodes.iter_mut().find(|n| &n.id == id) {
                    n.x = *x;
                    n.y = *y;
                }
            }
            CanvasPatchOp::NodeUpdate { node } => {
                if let Some(slot) = canvas.nodes.iter_mut().find(|n| n.id == node.id) {
                    *slot = node.clone();
                }
            }
            CanvasPatchOp::EdgeAdd { edge } => {
                if canvas.edges.iter().any(|e| e.id == edge.id) {
                    return Err(CanvasPatchError::DuplicateEdgeId(edge.id.clone()));
                }
                canvas.edges.push(edge.clone());
            }
            CanvasPatchOp::EdgeRemove { id } => {
                canvas.edges.retain(|e| &e.id != id);
            }
            CanvasPatchOp::EdgeUpdate { edge } => {
                if let Some(slot) = canvas.edges.iter_mut().find(|e| e.id == edge.id) {
                    *slot = edge.clone();
                }
            }
        }
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canvas_empty() {
        let canvas = parse_canvas(r#"{"nodes":[],"edges":[]}"#).unwrap();
        assert!(canvas.nodes.is_empty());
        assert!(canvas.edges.is_empty());
    }

    #[test]
    fn parse_canvas_with_text_node() {
        let json = r#"{
            "nodes": [{
                "id": "n1", "type": "text", "text": "Hello",
                "x": 10, "y": 20, "width": 300, "height": 200
            }],
            "edges": []
        }"#;
        let canvas = parse_canvas(json).unwrap();
        assert_eq!(canvas.nodes.len(), 1);
        assert_eq!(canvas.nodes[0].id, "n1");
        assert_eq!(canvas.nodes[0].node_type, CanvasNodeType::Text);
        assert_eq!(canvas.nodes[0].text.as_deref(), Some("Hello"));
        assert!((canvas.nodes[0].x - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_canvas_with_file_node() {
        let json = r#"{
            "nodes": [{
                "id": "n1", "type": "file", "file": "notes/design.md",
                "x": 0, "y": 0, "width": 250, "height": 300
            }],
            "edges": []
        }"#;
        let canvas = parse_canvas(json).unwrap();
        assert_eq!(canvas.nodes[0].node_type, CanvasNodeType::File);
        assert_eq!(canvas.nodes[0].file.as_deref(), Some("notes/design.md"));
    }

    #[test]
    fn parse_canvas_with_edges() {
        let json = r#"{
            "nodes": [
                {"id": "n1", "type": "text", "text": "A", "x": 0, "y": 0, "width": 100, "height": 100},
                {"id": "n2", "type": "text", "text": "B", "x": 200, "y": 0, "width": 100, "height": 100}
            ],
            "edges": [
                {"id": "e1", "from": "n1", "to": "n2", "label": "depends on", "type": "dashed"}
            ]
        }"#;
        let canvas = parse_canvas(json).unwrap();
        assert_eq!(canvas.edges.len(), 1);
        assert_eq!(canvas.edges[0].from_node, "n1");
        assert_eq!(canvas.edges[0].to_node, "n2");
        assert_eq!(canvas.edges[0].edge_type, CanvasEdgeType::Dashed);
        assert_eq!(canvas.edges[0].label.as_deref(), Some("depends on"));
    }

    #[test]
    fn parse_canvas_all_node_types() {
        let json = r#"{
            "nodes": [
                {"id": "n1", "type": "file", "file": "a.md", "x": 0, "y": 0, "width": 100, "height": 100},
                {"id": "n2", "type": "text", "text": "hi", "x": 0, "y": 0, "width": 100, "height": 100},
                {"id": "n3", "type": "link", "url": "https://example.com", "x": 0, "y": 0, "width": 100, "height": 100},
                {"id": "n4", "type": "group", "label": "G1", "x": 0, "y": 0, "width": 100, "height": 100},
                {"id": "n5", "type": "database", "source": "tasks.bases", "x": 0, "y": 0, "width": 100, "height": 100},
                {"id": "n6", "type": "terminal", "command": "cargo test", "x": 0, "y": 0, "width": 100, "height": 100}
            ],
            "edges": []
        }"#;
        let canvas = parse_canvas(json).unwrap();
        assert_eq!(canvas.nodes.len(), 6);
        assert_eq!(canvas.nodes[0].node_type, CanvasNodeType::File);
        assert_eq!(canvas.nodes[1].node_type, CanvasNodeType::Text);
        assert_eq!(canvas.nodes[2].node_type, CanvasNodeType::Link);
        assert_eq!(canvas.nodes[3].node_type, CanvasNodeType::Group);
        assert_eq!(canvas.nodes[4].node_type, CanvasNodeType::Database);
        assert_eq!(canvas.nodes[5].node_type, CanvasNodeType::Terminal);
    }

    #[test]
    fn parse_canvas_all_edge_types() {
        let json = r#"{
            "nodes": [],
            "edges": [
                {"id": "e1", "from": "a", "to": "b", "type": "solid"},
                {"id": "e2", "from": "a", "to": "c", "type": "dashed"},
                {"id": "e3", "from": "a", "to": "d", "type": "dotted"}
            ]
        }"#;
        let canvas = parse_canvas(json).unwrap();
        assert_eq!(canvas.edges[0].edge_type, CanvasEdgeType::Solid);
        assert_eq!(canvas.edges[1].edge_type, CanvasEdgeType::Dashed);
        assert_eq!(canvas.edges[2].edge_type, CanvasEdgeType::Dotted);
    }

    #[test]
    fn serialize_round_trip() {
        let original = CanvasFile {
            nodes: vec![CanvasNode {
                id: "n1".to_string(),
                node_type: CanvasNodeType::Text,
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
                color: Some("#FF0000".to_string()),
                label: None,
                collapsed: false,
                file: None,
                text: Some("Hello".to_string()),
                url: None,
                source: None,
                command: None,
            }],
            edges: vec![CanvasEdge {
                id: "e1".to_string(),
                from_node: "n1".to_string(),
                to_node: "n2".to_string(),
                edge_type: CanvasEdgeType::Dashed,
                label: Some("links to".to_string()),
                color: None,
            }],
            ..CanvasFile::default()
        };
        let json = serialize_canvas(&original).unwrap();
        let parsed = parse_canvas(&json).unwrap();
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "n1");
        assert_eq!(parsed.edges.len(), 1);
        assert_eq!(parsed.edges[0].edge_type, CanvasEdgeType::Dashed);
    }

    #[test]
    fn parse_canvas_invalid_json() {
        let result = parse_canvas("not json");
        assert!(result.is_err());
    }

    #[test]
    fn insert_and_query_canvas() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('test.canvas', 'canvas', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let file_id = conn.last_insert_rowid() as i64;

        let canvas = CanvasFile {
            nodes: vec![
                CanvasNode {
                    id: "n1".to_string(),
                    node_type: CanvasNodeType::Text,
                    x: 0.0, y: 0.0, width: 100.0, height: 100.0,
                    color: None, label: Some("Hello".to_string()), collapsed: false,
                    file: None, text: Some("content".to_string()), url: None, source: None, command: None,
                },
                CanvasNode {
                    id: "n2".to_string(),
                    node_type: CanvasNodeType::File,
                    x: 200.0, y: 0.0, width: 250.0, height: 300.0,
                    color: None, label: None, collapsed: false,
                    file: Some("notes/a.md".to_string()), text: None, url: None, source: None, command: None,
                },
            ],
            edges: vec![CanvasEdge {
                id: "e1".to_string(),
                from_node: "n1".to_string(),
                to_node: "n2".to_string(),
                edge_type: CanvasEdgeType::Solid,
                label: Some("references".to_string()),
                color: None,
            }],
            ..CanvasFile::default()
        };

        insert_canvas(&conn, file_id, &canvas).unwrap();

        let nodes = query_canvas_nodes(&conn, file_id).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].node_id, "n1");
        assert_eq!(nodes[0].node_type, "text");
        assert_eq!(nodes[1].node_id, "n2");
        assert_eq!(nodes[1].node_type, "file");

        let edges = query_canvas_edges(&conn, file_id).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_node, "n1");
        assert_eq!(edges[0].to_node, "n2");
        assert_eq!(edges[0].edge_type, "solid");
    }

    #[test]
    fn delete_canvas_removes_data() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO files (path, file_type, content_hash, size_bytes, created_at, modified_at)
             VALUES ('del.canvas', 'canvas', 'h1', 10, 0, 0);",
            [],
        )
        .unwrap();
        let file_id = conn.last_insert_rowid() as i64;

        let canvas = CanvasFile {
            nodes: vec![CanvasNode {
                id: "n1".to_string(),
                node_type: CanvasNodeType::Text,
                x: 0.0, y: 0.0, width: 100.0, height: 100.0,
                color: None, label: None, collapsed: false,
                file: None, text: Some("hi".to_string()), url: None, source: None, command: None,
            }],
            ..CanvasFile::default()
        };
        insert_canvas(&conn, file_id, &canvas).unwrap();
        delete_canvas(&conn, file_id).unwrap();

        assert!(query_canvas_nodes(&conn, file_id).unwrap().is_empty());
        assert!(query_canvas_edges(&conn, file_id).unwrap().is_empty());
    }

    #[test]
    fn extract_file_links_finds_file_nodes() {
        let canvas = CanvasFile {
            nodes: vec![
                CanvasNode {
                    id: "n1".to_string(),
                    node_type: CanvasNodeType::File,
                    x: 0.0, y: 0.0, width: 100.0, height: 100.0,
                    color: None, label: None, collapsed: false,
                    file: Some("notes/a.md".to_string()), text: None, url: None, source: None, command: None,
                },
                CanvasNode {
                    id: "n2".to_string(),
                    node_type: CanvasNodeType::Text,
                    x: 0.0, y: 0.0, width: 100.0, height: 100.0,
                    color: None, label: None, collapsed: false,
                    file: None, text: Some("hi".to_string()), url: None, source: None, command: None,
                },
            ],
            ..CanvasFile::default()
        };
        let links = extract_file_links(&canvas);
        assert_eq!(links, vec!["notes/a.md"]);
    }

    fn text_node(id: &str, x: f64, y: f64) -> CanvasNode {
        CanvasNode {
            id: id.to_string(),
            node_type: CanvasNodeType::Text,
            x, y, width: 100.0, height: 100.0,
            color: None, label: None, collapsed: false,
            file: None, text: Some("t".to_string()), url: None, source: None, command: None,
        }
    }

    fn edge(id: &str, from: &str, to: &str) -> CanvasEdge {
        CanvasEdge {
            id: id.to_string(),
            from_node: from.to_string(),
            to_node: to.to_string(),
            edge_type: CanvasEdgeType::Solid,
            label: None,
            color: None,
        }
    }

    #[test]
    fn apply_patch_node_add_remove_move() {
        let mut c = CanvasFile::default();
        apply_patch(&mut c, &[CanvasPatchOp::NodeAdd { node: text_node("a", 0.0, 0.0) }]).unwrap();
        assert_eq!(c.nodes.len(), 1);

        apply_patch(&mut c, &[CanvasPatchOp::NodeMove { id: "a".into(), x: 50.0, y: 60.0 }]).unwrap();
        assert!((c.nodes[0].x - 50.0).abs() < f64::EPSILON);
        assert!((c.nodes[0].y - 60.0).abs() < f64::EPSILON);

        apply_patch(&mut c, &[CanvasPatchOp::NodeRemove { id: "a".into() }]).unwrap();
        assert!(c.nodes.is_empty());
    }

    #[test]
    fn apply_patch_duplicate_node_add_errors() {
        let mut c = CanvasFile { nodes: vec![text_node("a", 0.0, 0.0)], ..Default::default() };
        let err = apply_patch(&mut c, &[CanvasPatchOp::NodeAdd { node: text_node("a", 1.0, 1.0) }])
            .unwrap_err();
        assert!(matches!(err, CanvasPatchError::DuplicateNodeId(ref id) if id == "a"));
    }

    #[test]
    fn apply_patch_node_remove_also_removes_incident_edges() {
        let mut c = CanvasFile {
            nodes: vec![text_node("a", 0.0, 0.0), text_node("b", 0.0, 0.0)],
            edges: vec![edge("e1", "a", "b")],
            ..Default::default()
        };
        apply_patch(&mut c, &[CanvasPatchOp::NodeRemove { id: "a".into() }]).unwrap();
        assert!(c.edges.is_empty(), "edge incident to removed node must be dropped");
    }

    #[test]
    fn apply_patch_unknown_id_is_noop() {
        let mut c = CanvasFile::default();
        apply_patch(&mut c, &[
            CanvasPatchOp::NodeRemove { id: "ghost".into() },
            CanvasPatchOp::NodeMove { id: "ghost".into(), x: 1.0, y: 1.0 },
            CanvasPatchOp::EdgeRemove { id: "ghost".into() },
        ]).unwrap();
        assert!(c.nodes.is_empty() && c.edges.is_empty());
    }

    #[test]
    fn apply_patch_edge_add_remove() {
        let mut c = CanvasFile {
            nodes: vec![text_node("a", 0.0, 0.0), text_node("b", 0.0, 0.0)],
            ..Default::default()
        };
        apply_patch(&mut c, &[CanvasPatchOp::EdgeAdd { edge: edge("e1", "a", "b") }]).unwrap();
        assert_eq!(c.edges.len(), 1);

        let err = apply_patch(&mut c, &[CanvasPatchOp::EdgeAdd { edge: edge("e1", "a", "b") }])
            .unwrap_err();
        assert!(matches!(err, CanvasPatchError::DuplicateEdgeId(_)));

        apply_patch(&mut c, &[CanvasPatchOp::EdgeRemove { id: "e1".into() }]).unwrap();
        assert!(c.edges.is_empty());
    }

    #[test]
    fn apply_patch_node_update_replaces_in_place() {
        let mut c = CanvasFile { nodes: vec![text_node("a", 0.0, 0.0)], ..Default::default() };
        let mut replacement = text_node("a", 999.0, 999.0);
        replacement.label = Some("updated".to_string());
        apply_patch(&mut c, &[CanvasPatchOp::NodeUpdate { node: replacement }]).unwrap();
        assert_eq!(c.nodes[0].label.as_deref(), Some("updated"));
        assert!((c.nodes[0].x - 999.0).abs() < f64::EPSILON);
    }

    #[test]
    fn edge_default_type_is_solid() {
        let json = r#"{
            "nodes": [],
            "edges": [{"id": "e1", "from": "a", "to": "b"}]
        }"#;
        let canvas = parse_canvas(json).unwrap();
        assert_eq!(canvas.edges[0].edge_type, CanvasEdgeType::Solid);
    }
}
