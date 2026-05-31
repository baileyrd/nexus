//! Error types for the nexus-formats crate.
//!
//! Top-level [`Error`] wraps per-subsystem sub-enums via `#[from]`.

/// Top-level result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Top-level error for `nexus-formats`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Markdown parse or resolve error.
    #[error(transparent)]
    Markdown(#[from] MarkdownError),

    /// Canvas format error.
    #[error(transparent)]
    Canvas(#[from] CanvasError),

    /// Config file error.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// Filename / slug utility error.
    #[error(transparent)]
    Util(#[from] UtilError),

    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Sub-errors ────────────────────────────────────────────────────────────────

/// Errors from the markdown pipeline.
#[derive(Debug, thiserror::Error)]
pub enum MarkdownError {
    /// YAML frontmatter is malformed.
    #[error("malformed frontmatter in '{file}': {reason}")]
    FrontmatterParse {
        /// Source file name (best-effort).
        file: String,
        /// Human-readable parse failure.
        reason: String,
    },

    /// Embed recursion depth exceeds the limit.
    #[error("embed depth limit exceeded (max {max}) in '{file}'")]
    EmbedDepthExceeded {
        /// File where the depth was exceeded.
        file: String,
        /// The configured maximum depth.
        max: usize,
    },

    /// A circular embed chain was detected.
    #[error("circular embed detected: {cycle:?}")]
    CircularEmbed {
        /// Paths that form the cycle.
        cycle: Vec<String>,
    },
}

/// Errors from the canvas format.
#[derive(Debug, thiserror::Error)]
pub enum CanvasError {
    /// The canvas JSON is invalid.
    #[error("invalid canvas JSON in '{path}': {reason}")]
    InvalidJson {
        /// File path.
        path: String,
        /// Serde error message.
        reason: String,
    },

    /// The required `version` field is absent.
    #[error("missing required field 'version' in canvas '{path}'")]
    MissingVersion {
        /// File path.
        path: String,
    },
}

/// Errors from config file loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A TOML config file is malformed.
    #[error("malformed TOML in '{path}': {reason}")]
    TomlParse {
        /// Path to the file.
        path: String,
        /// TOML error message.
        reason: String,
    },

    /// A JSON config file is malformed.
    #[error("malformed JSON in '{path}': {reason}")]
    JsonParse {
        /// Path to the file.
        path: String,
        /// Serde error message.
        reason: String,
    },

    /// An `${ENV_VAR}` placeholder references an undefined variable.
    #[error("undefined env var in config: '${{{name}}}'")]
    UndefinedEnvVar {
        /// The variable name (without `${}`).
        name: String,
    },
}

/// Errors from filename or slug utilities.
#[derive(Debug, thiserror::Error)]
pub enum UtilError {
    /// The filename contains forbidden characters or is reserved.
    #[error("invalid filename '{name}': {reason}")]
    InvalidFilename {
        /// The rejected filename.
        name: String,
        /// Why it was rejected.
        reason: String,
    },

    /// A path exceeds the platform maximum byte length.
    #[error("path exceeds maximum length ({max} bytes): '{path}'")]
    PathTooLong {
        /// The offending path.
        path: String,
        /// Maximum allowed length.
        max: usize,
    },
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_error_frontmatter_display() {
        let e = MarkdownError::FrontmatterParse {
            file: "note.md".into(),
            reason: "unexpected end".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("note.md"));
        assert!(msg.contains("unexpected end"));
    }

    #[test]
    fn markdown_error_embed_depth_display() {
        let e = MarkdownError::EmbedDepthExceeded {
            file: "a.md".into(),
            max: 10,
        };
        let msg = format!("{e}");
        assert!(msg.contains("10"));
        assert!(msg.contains("a.md"));
    }

    #[test]
    fn canvas_error_missing_version_display() {
        let e = CanvasError::MissingVersion {
            path: "board.canvas".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("board.canvas"));
        assert!(msg.contains("version"));
    }

    #[test]
    fn config_error_undefined_env_var_display() {
        let e = ConfigError::UndefinedEnvVar {
            name: "SECRET_KEY".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("SECRET_KEY"));
    }

    #[test]
    fn util_error_invalid_filename_display() {
        let e = UtilError::InvalidFilename {
            name: "CON".into(),
            reason: "reserved name".into(),
        };
        let msg = format!("{e}");
        assert!(msg.contains("CON"));
        assert!(msg.contains("reserved"));
    }

    #[test]
    fn top_level_error_wraps_canvas() {
        let canvas_err = CanvasError::MissingVersion {
            path: "x.canvas".into(),
        };
        let err: Error = canvas_err.into();
        assert!(matches!(err, Error::Canvas(_)));
    }
}
