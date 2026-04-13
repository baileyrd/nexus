//! Canvas format types (v1.0).
//!
//! Mirrors the Obsidian `.canvas` JSON schema with forward-compatibility via
//! `#[serde(flatten)]` on `extra` fields.

use serde::{Deserialize, Serialize};

/// A parsed `.canvas` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasFile {
    /// Format version — required, must be `"1.0"` for this reader.
    pub version: String,
    /// Nodes on the canvas.
    #[serde(default)]
    pub nodes: Vec<CanvasNode>,
    /// Edges connecting nodes.
    #[serde(default)]
    pub edges: Vec<CanvasEdge>,
    /// Unknown top-level fields are preserved for forward-compatibility.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    /// Source node ID.
    #[serde(rename = "from")]
    pub from_node: String,
    /// Target node ID.
    #[serde(rename = "to")]
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
}
