//! Retrieval-Augmented Generation (RAG) pipeline.
//!
//! Combines the vector store, embedding provider, and chat provider to
//! answer questions grounded in the user's personal knowledge base.

use std::fmt::Write as _;

use rusqlite::Connection;

use crate::chunker::chunks_from_blocks;
use crate::embedding::EmbeddingProvider;
use crate::error::AiError;
use crate::provider::{AiProvider, ChatMessage, Role};
use crate::vectorstore::{self, ChunkEmbedding, ChunkMatch};

/// Default maximum chunk size in characters.
const DEFAULT_MAX_CHUNK_SIZE: usize = 1024;

/// The response from a RAG query, including the generated answer and sources.
#[derive(Debug, Clone)]
pub struct RagResponse {
    /// The generated answer text.
    pub answer: String,
    /// The source chunks used to generate the answer.
    pub sources: Vec<ChunkMatch>,
    /// The name of the model that generated the answer.
    pub model: String,
}

/// Answer a question using retrieval-augmented generation.
///
/// Embeds the `question`, searches the vector store for relevant chunks,
/// builds a grounded system prompt with the retrieved context, and sends
/// the conversation to the AI provider.
///
/// # Errors
///
/// Returns [`AiError`] if embedding the question fails, the vector store
/// query fails, or the AI provider chat call fails.
pub async fn query(
    conn: &Connection,
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

    let sources = vectorstore::search(conn, &q_embedding, limit)?;
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

/// Index a file's blocks into the vector store.
///
/// Chunks the blocks, embeds all chunks in a single batch, and upserts
/// them into the `embeddings` table.  Returns the number of chunks stored.
///
/// # Errors
///
/// Returns [`AiError`] if embedding generation fails or the vector store
/// upsert fails.
pub async fn index_file(
    conn: &Connection,
    embedder: &dyn EmbeddingProvider,
    file_path: &str,
    blocks: &[(u64, String, String, Option<i32>)],
) -> Result<usize, AiError> {
    let chunks = chunks_from_blocks(file_path, blocks, DEFAULT_MAX_CHUNK_SIZE);

    if chunks.is_empty() {
        vectorstore::delete_by_file(conn, file_path)?;
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

    vectorstore::upsert(conn, file_path, &chunk_embeddings)?;

    Ok(chunk_embeddings.len())
}

/// Build the system prompt for the RAG conversation.
///
/// When no sources are available, returns a generic helpful-assistant
/// prompt.  Otherwise, enumerates the sources with `[[file_path]]`
/// citations so the model can reference them.
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
