//! Vector-store IPC client for `com.nexus.storage`.
//!
//! The AI plugin does not open its own `SQLite` connection — storage is the
//! sole owner of the forge database. These helpers issue `ipc_call`s to
//! storage's `vector_*` handlers. The [`ChunkEmbedding`] and [`ChunkMatch`]
//! types match the JSON shape emitted by `nexus_storage::vectorstore` so
//! responses deserialize directly.

use std::sync::Arc;

use nexus_kernel::{ipc_call, IpcDispatcher};
use serde::{Deserialize, Serialize};

use crate::error::AiError;

/// Plugin id of the storage core plugin.
const STORAGE_PLUGIN: &str = "com.nexus.storage";

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
}

/// A search hit from the vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Replace all embeddings for `file_path` via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] wrapping any dispatcher or handler error.
pub async fn upsert(
    dispatcher: &Arc<dyn IpcDispatcher>,
    file_path: &str,
    chunks: &[ChunkEmbedding],
) -> Result<(), AiError> {
    let args = serde_json::json!({ "file_path": file_path, "chunks": chunks });
    ipc_call(dispatcher, STORAGE_PLUGIN, "vector_insert", args)
        .await
        .map_err(|e| AiError::Provider(format!("storage vector_insert: {e}")))?;
    Ok(())
}

/// Search the vector store via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure or malformed response.
pub async fn search(
    dispatcher: &Arc<dyn IpcDispatcher>,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<ChunkMatch>, AiError> {
    let args = serde_json::json!({ "embedding": query_embedding, "limit": limit });
    let response = ipc_call(dispatcher, STORAGE_PLUGIN, "vector_query", args)
        .await
        .map_err(|e| AiError::Provider(format!("storage vector_query: {e}")))?;
    serde_json::from_value(response)
        .map_err(|e| AiError::Provider(format!("vector_query: decode: {e}")))
}

/// Delete all embeddings for `file_path` via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure.
pub async fn delete_by_file(
    dispatcher: &Arc<dyn IpcDispatcher>,
    file_path: &str,
) -> Result<(), AiError> {
    let args = serde_json::json!({ "path": file_path });
    ipc_call(dispatcher, STORAGE_PLUGIN, "vector_delete_by_file", args)
        .await
        .map_err(|e| AiError::Provider(format!("storage vector_delete_by_file: {e}")))?;
    Ok(())
}

/// Count all stored embeddings via storage IPC.
///
/// # Errors
/// Returns [`AiError::Provider`] on dispatcher failure or malformed response.
pub async fn count(dispatcher: &Arc<dyn IpcDispatcher>) -> Result<usize, AiError> {
    let response = ipc_call(
        dispatcher,
        STORAGE_PLUGIN,
        "vectorstore_count",
        serde_json::json!({}),
    )
    .await
    .map_err(|e| AiError::Provider(format!("storage vectorstore_count: {e}")))?;
    response
        .get("count")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| AiError::Provider("vectorstore_count: missing 'count'".into()))
}
