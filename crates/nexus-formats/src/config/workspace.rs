//! Workspace state (`workspace.json`).

use serde::{Deserialize, Serialize};

/// UI workspace state loaded from `.forge/workspace.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceState {
    /// Currently active file path (vault-relative).
    pub active_file: Option<String>,
    /// Open file list with cursor positions.
    pub open_files: Vec<OpenFileEntry>,
    /// Whether the sidebar is collapsed.
    pub sidebar_collapsed: bool,
    /// Panel layout configuration.
    pub panel_layout: PanelLayout,
    /// Recently opened files (most recent first).
    pub recent_files: Vec<String>,
    /// Last search query.
    pub search_query: String,
    /// Active theme name.
    pub theme: String,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            active_file: None,
            open_files: Vec::new(),
            sidebar_collapsed: false,
            panel_layout: PanelLayout::default(),
            recent_files: Vec::new(),
            search_query: String::new(),
            theme: "dark".into(),
        }
    }
}

/// An open file with its cursor position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFileEntry {
    /// Vault-relative file path.
    pub file: String,
    /// Cursor line (1-based).
    #[serde(default)]
    pub line: u32,
    /// Cursor column.
    #[serde(default)]
    pub column: u32,
}

/// Sidebar / panel layout dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelLayout {
    /// Left panel.
    pub left: PanelConfig,
    /// Right panel.
    pub right: PanelConfig,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            left: PanelConfig {
                width: 250,
                collapsed: false,
            },
            right: PanelConfig {
                width: 300,
                collapsed: true,
            },
        }
    }
}

/// Configuration for a single panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    /// Panel width in pixels.
    pub width: u32,
    /// Whether the panel is collapsed.
    pub collapsed: bool,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            width: 250,
            collapsed: false,
        }
    }
}
