//! Retrieval-Augmented Generation (RAG) pipeline.
//!
//! Combines HTTP-based embedding + chat providers with storage-owned vector
//! search (reached through `com.nexus.storage` IPC). The pipeline does not
//! touch `SQLite` directly.

use std::fmt::Write as _;

use nexus_kernel::KernelPluginContext;
use serde::{Deserialize, Serialize};

use crate::chunker::chunks_from_blocks;
use crate::embedding::EmbeddingProvider;
use crate::error::AiError;
use crate::provider::{AiProvider, ChatMessage, Role};
use crate::vectorstore::{self, ChunkEmbedding, ChunkMatch};

/// Default maximum chunk size in characters.
const DEFAULT_MAX_CHUNK_SIZE: usize = 1024;

/// The response from a RAG query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagResponse {
    /// Generated answer text.
    pub answer: String,
    /// Source chunks retrieved to ground the answer.
    pub sources: Vec<ChunkMatch>,
    /// Name of the model that generated the answer.
    pub model: String,
}

/// Answer a question using retrieval-augmented generation.
///
/// Embeds the question, fetches the top `limit` matching chunks via storage
/// IPC, builds a grounded system prompt, and calls the AI provider.
///
/// # Errors
/// Returns [`AiError`] if embedding, vector search, or the chat call fails.
pub async fn query(
    ctx: &KernelPluginContext,
    ai: &dyn AiProvider,
    embedder: &dyn EmbeddingProvider,
    question: &str,
    limit: usize,
) -> Result<RagResponse, AiError> {
    let q_embeddings = embedder.embed(&[question.to_string()]).await?;
    let q_embedding = q_embeddings
        .into_iter()
        .next()
        .ok_or_else(|| AiError::Provider("embedding returned no vectors".into()))?;

    let sources = vectorstore::search(ctx, &q_embedding, limit).await?;
    let system = build_rag_prompt(&sources);

    let messages = vec![ChatMessage {
        role: Role::User,
        content: question.to_string(),
    }];

    let answer = ai.chat(&messages, Some(&system)).await?;

    Ok(RagResponse {
        answer,
        sources,
        model: ai.model_name().to_string(),
    })
}

/// Index a file's blocks by chunking, embedding, and upserting via storage
/// IPC. Returns the number of chunks stored.
///
/// # Errors
/// Returns [`AiError`] if embedding or the storage call fails.
pub async fn index_file(
    ctx: &KernelPluginContext,
    embedder: &dyn EmbeddingProvider,
    file_path: &str,
    blocks: &[(u64, String, String, Option<i32>)],
) -> Result<usize, AiError> {
    let chunks = chunks_from_blocks(file_path, blocks, DEFAULT_MAX_CHUNK_SIZE);

    if chunks.is_empty() {
        vectorstore::delete_by_file(ctx, file_path).await?;
        return Ok(0);
    }

    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let embeddings = embedder.embed(&texts).await?;

    let chunk_embeddings: Vec<ChunkEmbedding> = chunks
        .into_iter()
        .zip(embeddings)
        .map(|(chunk, emb)| ChunkEmbedding {
            file_path: chunk.file_path,
            block_id: chunk.block_id,
            chunk_text: chunk.content,
            embedding: emb,
        })
        .collect();

    let n = chunk_embeddings.len();
    vectorstore::upsert(ctx, file_path, &chunk_embeddings).await?;
    Ok(n)
}

/// Build the system prompt for the RAG conversation.
fn build_rag_prompt(sources: &[ChunkMatch]) -> String {
    if sources.is_empty() {
        return "You are a helpful assistant. Answer the user's question to the best of your ability.".to_string();
    }

    let mut prompt = String::from(
        "Use the following context from the user's notes to answer their question. \
         Cite sources using [[file_path]] notation when relevant.\n\n",
    );

    for (i, source) in sources.iter().enumerate() {
        let _ = write!(
            prompt,
            "Source {}: [[{}]]\n{}\n\n",
            i + 1,
            source.file_path,
            source.chunk_text,
        );
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_rag_prompt_with_no_sources() {
        let prompt = build_rag_prompt(&[]);
        assert!(prompt.contains("helpful assistant"));
    }

    #[test]
    fn build_rag_prompt_with_sources() {
        let sources = vec![
            ChunkMatch {
                file_path: "notes/rust.md".into(),
                block_id: 1,
                chunk_text: "Rust is a systems programming language.".into(),
                score: 0.95,
            },
            ChunkMatch {
                file_path: "notes/go.md".into(),
                block_id: 2,
                chunk_text: "Go is great for servers.".into(),
                score: 0.80,
            },
        ];
        let prompt = build_rag_prompt(&sources);
        assert!(prompt.contains("[[notes/rust.md]]"));
        assert!(prompt.contains("[[notes/go.md]]"));
        assert!(prompt.contains("Source 1"));
        assert!(prompt.contains("Source 2"));
    }
}
