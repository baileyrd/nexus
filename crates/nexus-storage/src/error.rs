//! Storage error types.

/// Errors from the storage subsystem.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// File not found at the given path.
    #[error("file not found: {0}")]
    FileNotFound(String),

    /// Permission denied accessing path.
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// File failed to parse (corrupt or unsupported format).
    #[error("corrupt file {path}: {reason}")]
    CorruptFile {
        /// Path to the corrupt file.
        path: String,
        /// What went wrong.
        reason: String,
    },

    /// Index state doesn't match filesystem state.
    #[error("index inconsistency: {details}")]
    IndexInconsistency {
        /// Description of the inconsistency.
        details: String,
    },

    /// Atomic write failed after retries.
    #[error("write failed for {path}: {reason}")]
    WriteFailed {
        /// Target path that could not be written.
        path: String,
        /// Reason for failure.
        reason: String,
    },

    /// Markdown/MDX parse error.
    #[error("parse error in {file}: {error}")]
    ParseError {
        /// File that failed to parse.
        file: String,
        /// Parser error details.
        error: String,
    },

    /// Another process holds the forge lock.
    #[error("forge locked by another process: {0}")]
    LockHeld(String),

    /// Configuration is invalid.
    #[error("invalid configuration: {0}")]
    ConfigInvalid(String),

    /// `SQLite` error.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Tantivy search error.
    #[error("search error: {0}")]
    Search(#[from] tantivy::TantivyError),

    /// File watcher error.
    #[error("watcher error: {0}")]
    Watcher(#[from] notify::Error),

    /// Bases filesystem error.
    #[error("bases error: {0}")]
    Bases(#[from] nexus_types::bases::BasesError),

    /// Config file error from `nexus-formats`.
    #[error("config error: {0}")]
    Config(#[from] nexus_formats::ConfigError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_not_found_display() {
        let err = StorageError::FileNotFound("notes/missing.md".to_string());
        assert_eq!(err.to_string(), "file not found: notes/missing.md");
    }

    #[test]
    fn permission_denied_display() {
        let err = StorageError::PermissionDenied("/root/secret".to_string());
        assert_eq!(err.to_string(), "permission denied: /root/secret");
    }

    #[test]
    fn io_error_wraps() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = StorageError::Io(io_err);
        assert!(err.to_string().contains("gone"));
    }

    #[test]
    fn corrupt_file_display() {
        let err = StorageError::CorruptFile {
            path: "bad.md".to_string(),
            reason: "invalid UTF-8".to_string(),
        };
        assert_eq!(err.to_string(), "corrupt file bad.md: invalid UTF-8");
    }

    #[test]
    fn index_inconsistency_display() {
        let err = StorageError::IndexInconsistency {
            details: "orphan block".to_string(),
        };
        assert_eq!(err.to_string(), "index inconsistency: orphan block");
    }

    #[test]
    fn write_failed_display() {
        let err = StorageError::WriteFailed {
            path: "notes/x.md".to_string(),
            reason: "disk full".to_string(),
        };
        assert_eq!(err.to_string(), "write failed for notes/x.md: disk full");
    }

    #[test]
    fn parse_error_display() {
        let err = StorageError::ParseError {
            file: "notes/y.md".to_string(),
            error: "unterminated frontmatter".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "parse error in notes/y.md: unterminated frontmatter"
        );
    }

    #[test]
    fn lock_held_display() {
        let err = StorageError::LockHeld("pid 1234".to_string());
        assert_eq!(err.to_string(), "forge locked by another process: pid 1234");
    }

    #[test]
    fn config_invalid_display() {
        let err = StorageError::ConfigInvalid("bad pool_size".to_string());
        assert_eq!(err.to_string(), "invalid configuration: bad pool_size");
    }

    #[test]
    fn io_error_converts_via_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let err: StorageError = io_err.into();
        assert!(matches!(err, StorageError::Io(_)));
    }
}
