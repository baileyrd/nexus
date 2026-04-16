//! Canvas format parser and serializer.
//!
//! The `.canvas` format is a JSON file containing nodes and edges.
//! The `version` field defaults to `"1.0"` when absent for
//! backward-compatibility with legacy and test files.

mod types;

pub use types::{CanvasEdge, CanvasEdgeType, CanvasFile, CanvasNode, CanvasNodeType};

use crate::error::CanvasError;

/// Parse a `.canvas` JSON string into a [`CanvasFile`].
///
/// The `version` field defaults to `"1.0"` when absent.
///
/// # Errors
///
/// Returns [`CanvasError::InvalidJson`] if the JSON is malformed.
pub fn parse(json: &str) -> Result<CanvasFile, CanvasError> {
    serde_json::from_str(json).map_err(|e| CanvasError::InvalidJson {
        path: "<canvas>".to_string(),
        reason: e.to_string(),
    })
}

/// Parse a `.canvas` JSON string, specifying a path for error messages.
///
/// # Errors
///
/// Returns [`CanvasError::InvalidJson`] if the JSON is malformed.
pub fn parse_with_path(json: &str, path: &str) -> Result<CanvasFile, CanvasError> {
    serde_json::from_str(json).map_err(|e| CanvasError::InvalidJson {
        path: path.to_string(),
        reason: e.to_string(),
    })
}

/// Serialize a [`CanvasFile`] to pretty-printed JSON.
///
/// # Errors
///
/// Returns [`CanvasError::InvalidJson`] on serialization failure (extremely rare).
pub fn serialize(canvas: &CanvasFile) -> Result<String, CanvasError> {
    serde_json::to_string_pretty(canvas).map_err(|e| CanvasError::InvalidJson {
        path: "<canvas>".to_string(),
        reason: e.to_string(),
    })
}

/// Extract all vault-relative file paths referenced by `file`-type nodes.
#[must_use]
pub fn file_links(canvas: &CanvasFile) -> Vec<String> {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_canvas(extra: &str) -> String {
        format!(r#"{{"version":"1.0","nodes":[],"edges":[]{extra}}}"#)
    }

    #[test]
    fn parse_minimal() {
        let c = parse(&minimal_canvas("")).unwrap();
        assert_eq!(c.version, "1.0");
        assert!(c.nodes.is_empty());
        assert!(c.edges.is_empty());
    }

    #[test]
    fn parse_missing_version_defaults_to_1_0() {
        let json = r#"{"nodes":[],"edges":[]}"#;
        let c = parse(json).unwrap();
        assert_eq!(c.version, "1.0");
    }

    #[test]
    fn parse_invalid_json_errors() {
        let err = parse("not json").unwrap_err();
        assert!(matches!(err, CanvasError::InvalidJson { .. }));
    }

    #[test]
    fn parse_all_node_types() {
        let json = r#"{
            "version":"1.0",
            "nodes":[
                {"id":"n1","type":"file","file":"a.md","x":0,"y":0,"width":100,"height":100},
                {"id":"n2","type":"text","text":"hi","x":0,"y":0,"width":100,"height":100},
                {"id":"n3","type":"link","url":"https://example.com","x":0,"y":0,"width":100,"height":100},
                {"id":"n4","type":"group","label":"G1","x":0,"y":0,"width":100,"height":100},
                {"id":"n5","type":"database","source":"tasks.bases","x":0,"y":0,"width":100,"height":100},
                {"id":"n6","type":"terminal","command":"cargo test","x":0,"y":0,"width":100,"height":100}
            ],
            "edges":[]
        }"#;
        let c = parse(json).unwrap();
        assert_eq!(c.nodes.len(), 6);
        assert_eq!(c.nodes[0].node_type, CanvasNodeType::File);
        assert_eq!(c.nodes[5].node_type, CanvasNodeType::Terminal);
    }

    #[test]
    fn parse_all_edge_types() {
        let json = r#"{
            "version":"1.0","nodes":[],
            "edges":[
                {"id":"e1","from":"a","to":"b","type":"solid"},
                {"id":"e2","from":"a","to":"c","type":"dashed"},
                {"id":"e3","from":"a","to":"d","type":"dotted"}
            ]
        }"#;
        let c = parse(json).unwrap();
        assert_eq!(c.edges[0].edge_type, CanvasEdgeType::Solid);
        assert_eq!(c.edges[1].edge_type, CanvasEdgeType::Dashed);
        assert_eq!(c.edges[2].edge_type, CanvasEdgeType::Dotted);
    }

    #[test]
    fn edge_default_type_is_solid() {
        let json = r#"{"version":"1.0","nodes":[],"edges":[{"id":"e1","from":"a","to":"b"}]}"#;
        let c = parse(json).unwrap();
        assert_eq!(c.edges[0].edge_type, CanvasEdgeType::Solid);
    }

    #[test]
    fn unknown_fields_preserved_in_extra() {
        let json = r#"{"version":"1.0","nodes":[],"edges":[],"futureField":"value"}"#;
        let c = parse(json).unwrap();
        assert!(c.extra.contains_key("futureField"));
    }

    #[test]
    fn serialize_round_trip() {
        let original = CanvasFile {
            version: "1.0".to_string(),
            nodes: vec![CanvasNode {
                id: "n1".to_string(),
                node_type: CanvasNodeType::Text,
                x: 10.0, y: 20.0, width: 300.0, height: 200.0,
                color: None, label: None, collapsed: false,
                file: None, text: Some("Hello".into()), url: None, source: None, command: None,
            }],
            edges: vec![CanvasEdge {
                id: "e1".to_string(),
                from_node: "n1".into(), to_node: "n2".into(),
                edge_type: CanvasEdgeType::Dashed,
                label: Some("links to".into()), color: None,
            }],
            extra: serde_json::Map::new(),
        };
        let json_str = serialize(&original).unwrap();
        let parsed = parse(&json_str).unwrap();
        assert_eq!(parsed.nodes[0].id, "n1");
        assert_eq!(parsed.edges[0].edge_type, CanvasEdgeType::Dashed);
    }

    #[test]
    fn file_links_extracts_file_nodes() {
        let json = r#"{
            "version":"1.0",
            "nodes":[
                {"id":"n1","type":"file","file":"notes/a.md","x":0,"y":0,"width":100,"height":100},
                {"id":"n2","type":"text","text":"hi","x":0,"y":0,"width":100,"height":100}
            ],
            "edges":[]
        }"#;
        let c = parse(json).unwrap();
        let links = file_links(&c);
        assert_eq!(links, vec!["notes/a.md"]);
    }
}
