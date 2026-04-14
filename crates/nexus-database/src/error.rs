//! Error types for the database engine.

/// Top-level error type for `nexus-database`.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    /// A field value failed type-aware validation.
    #[error("validation failed for field '{field}': {reason}")]
    ValidationFailed {
        /// The field that failed validation.
        field: String,
        /// Human-readable reason.
        reason: String,
    },

    /// A schema operation failed (add/remove/rename property, migration).
    #[error("schema error: {0}")]
    SchemaError(String),

    /// A query failed to compile or execute.
    #[error("query error: {0}")]
    QueryError(String),

    /// A formula failed to parse or evaluate.
    #[error("formula error at position {position}: {message}")]
    FormulaError {
        /// Character position in the formula source where the error occurred.
        position: usize,
        /// Human-readable error message.
        message: String,
    },

    /// A relation resolution or rollup aggregation failed.
    #[error("relation error: {0}")]
    RelationError(String),

    /// An import or export operation failed.
    #[error("import/export error: {0}")]
    ImportExportError(String),

    /// Propagated from the bases filesystem layer.
    #[error(transparent)]
    Bases(#[from] nexus_types::bases::BasesError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience result type for the database engine.
pub type Result<T> = std::result::Result<T, DatabaseError>;
