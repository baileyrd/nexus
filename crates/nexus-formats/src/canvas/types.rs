//! Canvas format types (v1.0).
//!
//! Mirrors the Obsidian `.canvas` JSON schema with forward-compatibility via
//! `#[serde(flatten)]` on `extra` fields.

use serde::{Deserialize, Serialize};

fn default_canvas_version() -> String {
    "1.0".to_string()
}

/// A parsed `.canvas` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasFile {
    /// Format version. Defaults to `"1.0"` when absent (legacy / test files).
    #[serde(default = "default_canvas_version")]
    pub version: String,
    /// Nodes on the canvas.
    #[serde(default)]
    pub nodes: Vec<CanvasNode>,
    /// Edges connecting nodes.
    #[serde(default)]
    pub edges: Vec<CanvasEdge>,
    /// Unknown top-level fields preserved for forward-compatibility.
    /// Skipped during serialization when empty.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for CanvasFile {
    fn default() -> Self {
        Self {
            version: default_canvas_version(),
            nodes: Vec::new(),
            edges: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }
}

/// A node placed on the canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasNode {
    /// Unique node identifier within this canvas.
    pub id: String,
    /// Node type discriminant.
    #[serde(rename = "type")]
    pub node_type: CanvasNodeType,
    /// Horizontal position.
    pub x: f64,
    /// Vertical position.
    pub y: f64,
    /// Node width.
    pub width: f64,
    /// Node height.
    pub height: f64,
    /// Optional display color (hex string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Optional display label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Whether the node is collapsed.
    #[serde(default)]
    pub collapsed: bool,

    // ── Type-specific fields ──────────────────────────────────────────────
    /// Vault-relative file path for `file` nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Text content for `text` nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// URL for `link` nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Source `.bases` path for `database` nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Shell command for `terminal` nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Unknown fields preserved for forward-compatibility (e.g. Obsidian's
    /// `subpath` on file nodes, or `styleAttributes`). Allows nodes
    /// authored in other JSON Canvas 1.0 implementations to round-trip
    /// through Nexus without losing data.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Discriminant for canvas node types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanvasNodeType {
    /// Embeds a vault file.
    File,
    /// Free-form text card.
    Text,
    /// External URL card.
    Link,
    /// Container for organizing other nodes.
    Group,
    /// Reference to a `.bases` database.
    Database,
    /// Interactive terminal / code execution block.
    Terminal,
}

impl CanvasNodeType {
    /// String representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File     => "file",
            Self::Text     => "text",
            Self::Link     => "link",
            Self::Group    => "group",
            Self::Database => "database",
            Self::Terminal => "terminal",
        }
    }
}

/// An edge connecting two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasEdge {
    /// Unique edge identifier.
    pub id: String,
    /// Source node ID. Accepts `fromNode` (JSON Canvas 1.0 spec, used by
    /// Obsidian) on read and also the legacy `from` key produced by
    /// older Nexus writes. Serialized as `fromNode` going forward.
    #[serde(rename = "fromNode", alias = "from")]
    pub from_node: String,
    /// Target node ID. Accepts `toNode` (spec) + `to` (legacy) on read;
    /// serialized as `toNode`.
    #[serde(rename = "toNode", alias = "to")]
    pub to_node: String,
    /// Edge line style (defaults to solid).
    #[serde(rename = "type", default = "default_edge_type")]
    pub edge_type: CanvasEdgeType,
    /// Optional relationship label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional display color.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Unknown edge fields (e.g. JSON Canvas 1.0's `fromSide` /
    /// `toSide` / `fromEnd` / `toEnd`) preserved for round-trip.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_edge_type() -> CanvasEdgeType {
    CanvasEdgeType::Solid
}

/// Edge line style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanvasEdgeType {
    /// Solid line.
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
}

impl CanvasEdgeType {
    /// String representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Solid  => "solid",
            Self::Dashed => "dashed",
            Self::Dotted => "dotted",
        }
    }

    /// Parse a string into an edge type, defaulting to [`Solid`](Self::Solid)
    /// for unrecognised values.
    #[must_use]
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "dashed" => Self::Dashed,
            "dotted" => Self::Dotted,
            _ => Self::Solid,
        }
    }
}
