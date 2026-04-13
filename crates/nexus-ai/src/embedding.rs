//! Embedding provider trait for vector representations of text.

use async_trait::async_trait;

use crate::error::AiError;

/// Trait for text embedding providers.
///
/// Implementors convert text into dense vector representations suitable
/// for semantic similarity search and retrieval-augmented generation.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embeddings for a batch of texts.
    ///
    /// Returns one vector per input text. All vectors share the same
    /// dimensionality as reported by [`EmbeddingProvider::dimension`].
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError>;

    /// Return the dimensionality of the embedding vectors produced.
    fn dimension(&self) -> usize;
}
