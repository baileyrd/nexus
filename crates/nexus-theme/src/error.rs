//! Error types for the theming engine.

use std::path::PathBuf;

/// Errors produced by the theming engine.
#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    /// I/O error reading a theme or snippet file.
    #[error("io error at {path}: {source}")]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Theme manifest (`NEXUS.toml`) failed to parse.
    #[error("failed to parse theme manifest at {path}: {source}")]
    Manifest {
        /// Manifest path.
        path: PathBuf,
        /// Underlying TOML parse error.
        #[source]
        source: toml::de::Error,
    },

    /// CSS snippet header failed to parse (missing required field etc.).
    #[error("invalid snippet header in {path}: {reason}")]
    SnippetHeader {
        /// Snippet path.
        path: PathBuf,
        /// Explanation.
        reason: String,
    },

    /// Requested theme id not registered with the engine.
    #[error("theme not found: {0}")]
    ThemeNotFound(String),

    /// Requested snippet id not registered with the engine.
    #[error("snippet not found: {0}")]
    SnippetNotFound(String),

    /// Variable name violates the `--nx-*` naming convention.
    #[error("invalid variable name (must start with `--nx-`): {0}")]
    InvalidVariableName(String),

    /// Circular `var(...)` reference during substitution.
    #[error("circular variable reference in {0}")]
    CircularReference(String),

    /// Pane id not present in the layout tree.
    #[error("pane not found in layout: {0}")]
    PaneNotFound(String),

    /// Tab id not present in any pane's tab list.
    #[error("tab not found in layout: {0}")]
    TabNotFound(String),

    /// Split sizes length did not match the number of children, or did not
    /// sum to approximately 1.0.
    #[error("invalid split sizes (children={children}, sizes={sizes:?}): {reason}")]
    InvalidSplitSizes {
        /// Expected number of children.
        children: usize,
        /// Sizes the caller supplied.
        sizes: Vec<f32>,
        /// Human-readable reason.
        reason: &'static str,
    },

    /// Attempt to mutate a split node as if it were a leaf, or vice versa.
    #[error("node {id} is not a {expected} (got {actual})")]
    NodeKindMismatch {
        /// Pane id the caller supplied.
        id: String,
        /// The node kind the caller asked for.
        expected: &'static str,
        /// The node kind actually found.
        actual: &'static str,
    },

    /// JSON (de)serialization error for the workspace layout.
    #[error("layout JSON error at {path}: {source}")]
    LayoutJson {
        /// Path if known.
        path: std::path::PathBuf,
        /// Underlying `serde_json` error.
        #[source]
        source: serde_json::Error,
    },

    /// TOML parsing error for a layout preset file.
    #[error("failed to parse layout preset at {path}: {source}")]
    PresetToml {
        /// Source path or synthetic marker (`"<embedded:obsidian>"`).
        path: PathBuf,
        /// Underlying TOML parse error.
        #[source]
        source: toml::de::Error,
    },

    /// Requested layout preset id not registered.
    #[error("layout preset not found: {0}")]
    PresetNotFound(String),
}

impl ThemeError {
    /// Construct an [`Io`](Self::Io) error with path context.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
