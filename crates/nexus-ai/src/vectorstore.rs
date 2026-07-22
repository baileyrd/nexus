//! Vector-store IPC client for `com.nexus.storage`.
//!
//! The AI plugin does not open its own `SQLite` connection — storage is the
//! sole owner of the forge database. These helpers issue `ipc_call`s via
//! the AI plugin's [`KernelPluginContext`] to storage's `vector_*` handlers.
//! The [`ChunkEmbedding`] and [`ChunkMatch`] types match the JSON shape
//! emitted by `nexus_storage::vectorstore` so responses deserialize directly.

use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::error::AiError;

/// Plugin id of the storage core plugin.
const STORAGE_PLUGIN: &str = "com.nexus.storage";

/// Timeout applied to every nested storage `ipc_call` from the AI plugin.
/// These are local `SQLite` queries — 30s is an extreme upper bound.
const STORAGE_IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// A chunk together with its embedding vector, ready for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEmbedding {
    /// Path of the source file.
    pub file_path: String,
    /// Identifier of the originating block.
    pub block_id: u64,
    /// The textual content of the chunk.
    pub chunk_text: String,
    /// Dense vector representation of the chunk.
    pub embedding: Vec<f32>,
    /// C19 (#372) — hash of the source file's content as of this embed
    /// pass, shared by every chunk from the same file.
    #[serde(default)]
    pub content_hash: Option<String>,
}

/// A search hit from the vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ChunkMatch {
    /// Path of the source file.
    pub file_path: String,
    /// Identifier of the originating block.
    pub block_id: u64,
    /// The textual content of the chunk.
    pub chunk_text: String,
    /// Cosine similarity score (higher is more relevant).
    pub score: f32,
}

/// Maximum number of chunks accepted in a single `upsert` call.
/// Issue #85. The audit flagged that there was no per-call cap on
/// the chunks slice; an indexer bug or malicious caller could
/// shove millions of embeddings through `vector_insert` in one
/// IPC dispatch and pin storage's write-side mutex for the
/// duration. 4096 is generous (a typical document chunks to a
/// few dozen blocks; a whole forge re-index batches across
/// many files), and the caller can split larger work into
/// multiple `upsert` calls.
pub const MAX_CHUNKS_PER_UPSERT: usize = 4096;

/// Maximum byte size of a single chunk's text. Embeddings cost
/// scales with chunk size; a multi-megabyte single chunk is a
/// signal the chunker is broken.
pub const MAX_CHUNK_TEXT_BYTES: usize = 256 * 1024;

/// Maximum embedding-vector length. The biggest production
/// embedding model (text-embedding-3-large) emits 3072-dim
/// vectors; padding 4× as headroom catches the "I forgot to
/// truncate" bug shape without rejecting any real model.
pub const MAX_EMBEDDING_DIM: usize = 12_288;

/// Replace all embeddings for `file_path` via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] wrapping any dispatcher or handler error,
/// or if `chunks` exceeds [`MAX_CHUNKS_PER_UPSERT`] / any chunk's text
/// exceeds [`MAX_CHUNK_TEXT_BYTES`] / any embedding length exceeds
/// [`MAX_EMBEDDING_DIM`].
pub async fn upsert(
    ctx: &KernelPluginContext,
    file_path: &str,
    chunks: &[ChunkEmbedding],
) -> Result<(), AiError> {
    if chunks.len() > MAX_CHUNKS_PER_UPSERT {
        return Err(AiError::Provider(format!(
            "vector_insert: {} chunks; max is {MAX_CHUNKS_PER_UPSERT}",
            chunks.len()
        )));
    }
    for (i, chunk) in chunks.iter().enumerate() {
        if chunk.chunk_text.len() > MAX_CHUNK_TEXT_BYTES {
            return Err(AiError::Provider(format!(
                "vector_insert: chunk {i} text is {} bytes; max is {MAX_CHUNK_TEXT_BYTES}",
                chunk.chunk_text.len()
            )));
        }
        if chunk.embedding.len() > MAX_EMBEDDING_DIM {
            return Err(AiError::Provider(format!(
                "vector_insert: chunk {i} embedding is {} dims; max is {MAX_EMBEDDING_DIM}",
                chunk.embedding.len()
            )));
        }
    }
    let args = serde_json::json!({ "file_path": file_path, "chunks": chunks });
    ctx.ipc_call(STORAGE_PLUGIN, "vector_insert", args, STORAGE_IPC_TIMEOUT)
        .await
        .map_err(|e| AiError::Provider(format!("storage vector_insert: {e}")))?;
    Ok(())
}

/// Search the vector store via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure or malformed response.
pub async fn search(
    ctx: &KernelPluginContext,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<ChunkMatch>, AiError> {
    let args = serde_json::json!({ "embedding": query_embedding, "limit": limit });
    let response = ctx
        .ipc_call(STORAGE_PLUGIN, "vector_query", args, STORAGE_IPC_TIMEOUT)
        .await
        .map_err(|e| AiError::Provider(format!("storage vector_query: {e}")))?;
    serde_json::from_value(response)
        .map_err(|e| AiError::Provider(format!("vector_query: decode: {e}")))
}

/// Fused keyword + vector query via storage's `hybrid_search` (handler
/// id `76`) — reciprocal-rank fusion of the Tantivy FTS arm and the
/// vector arm. Returns the fused match objects exactly as storage
/// serialises them (`StorageHybridMatch` wire shape: `file_path`,
/// `block_id`, `block_type`, `excerpt`, `score`, `fts_rank`,
/// `vector_rank`) so the handler can pass them through without a
/// mirror struct on this side.
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure or a malformed
/// reply.
pub async fn hybrid_search(
    ctx: &KernelPluginContext,
    query: &str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<serde_json::Value>, AiError> {
    let args = serde_json::json!({
        "query": query,
        "embedding": query_embedding,
        "limit": limit,
    });
    let response = ctx
        .ipc_call(STORAGE_PLUGIN, "hybrid_search", args, STORAGE_IPC_TIMEOUT)
        .await
        .map_err(|e| AiError::Provider(format!("storage hybrid_search: {e}")))?;
    response
        .get("results")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .ok_or_else(|| {
            AiError::Provider("hybrid_search: reply missing 'results' array".to_string())
        })
}

/// Delete all embeddings for `file_path` via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure.
pub async fn delete_by_file(ctx: &KernelPluginContext, file_path: &str) -> Result<(), AiError> {
    let args = serde_json::json!({ "path": file_path });
    ctx.ipc_call(
        STORAGE_PLUGIN,
        "vector_delete_by_file",
        args,
        STORAGE_IPC_TIMEOUT,
    )
    .await
    .map_err(|e| AiError::Provider(format!("storage vector_delete_by_file: {e}")))?;
    Ok(())
}

/// C19 (#372) — the `(content_hash, embedding_dim)` already stored for
/// `file_path` via storage IPC, or `None` if nothing usable is stored
/// yet (never embedded, or embedded before this feature shipped).
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure or malformed response.
pub async fn stored_signature(
    ctx: &KernelPluginContext,
    file_path: &str,
) -> Result<Option<(String, usize)>, AiError> {
    let args = serde_json::json!({ "file_path": file_path });
    let response = ctx
        .ipc_call(
            STORAGE_PLUGIN,
            "vector_stored_signature",
            args,
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| AiError::Provider(format!("storage vector_stored_signature: {e}")))?;
    let content_hash = response
        .get("content_hash")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let embedding_dim = response
        .get("embedding_dim")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok());
    Ok(content_hash.zip(embedding_dim))
}

/// Count all stored embeddings via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure or malformed response.
pub async fn count(ctx: &KernelPluginContext) -> Result<usize, AiError> {
    let response = ctx
        .ipc_call(
            STORAGE_PLUGIN,
            "vectorstore_count",
            serde_json::json!({}),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| AiError::Provider(format!("storage vectorstore_count: {e}")))?;
    response
        .get("count")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| AiError::Provider("vectorstore_count: missing 'count'".into()))
}
