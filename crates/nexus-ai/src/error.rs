//! Error types for the AI engine.

use thiserror::Error;

/// Errors that can occur during AI operations.
#[derive(Debug, Error)]
pub enum AiError {
    /// Authentication failed (invalid or missing API key).
    #[error("authentication failed: {0}")]
    AuthFailed(String),

    /// Network or HTTP transport error.
    #[error("network error: {0}")]
    Network(String),

    /// Provider-specific error (non-auth API errors).
    #[error("provider error: {0}")]
    Provider(String),

    /// No chat provider is configured or detected.
    #[error("no AI provider configured")]
    NoProvider,

    /// No embedding provider is configured or detected.
    #[error("no embedding provider configured")]
    NoEmbeddingProvider,

    /// Database error from rusqlite.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Serialization or deserialization error.
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl From<reqwest::Error> for AiError {
    fn from(err: reqwest::Error) -> Self {
        Self::Network(err.to_string())
    }
}
