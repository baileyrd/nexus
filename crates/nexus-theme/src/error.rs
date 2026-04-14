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
