//! Vector-store handlers: `vector_insert`, `vector_query`,
//! `vector_delete_by_file`, `vectorstore_count`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    StorageChunkEmbedding, StorageOk, StorageVectorCountArgs, StorageVectorDeleteArgs,
    StorageVectorInsertArgs, StorageVectorQueryArgs, StorageVectorstoreCountResult,
};
use crate::StorageEngine;

use super::shared::{exec_err, parse_args, to_value};

/// Default match count when the caller omits `limit`. Kept identical
/// to the pre-#190 hand-rolled default so the migration is wire-
/// compatible with existing callers.
const DEFAULT_QUERY_LIMIT: usize = 5;

pub(crate) fn insert(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `StorageVectorInsertArgs`.
    let StorageVectorInsertArgs {
        namespace,
        file_path,
        chunks,
    } = parse_args(args, "vector_insert")?;
    let impl_chunks: Vec<crate::vectorstore::ChunkEmbedding> =
        chunks.into_iter().map(chunk_to_impl).collect();
    engine
        .vector_insert(&namespace, &file_path, &impl_chunks)
        .map_err(|e| exec_err(format!("vector_insert: {e}")))?;
    to_value(&StorageOk { ok: true }, "vector_insert")
}

pub(crate) fn query(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `StorageVectorQueryArgs`.
    // The reply still serializes `Vec<ChunkMatch>`; its wire shape
    // matches `Vec<StorageVectorMatch>` field-for-field (compared
    // via `cargo test -p nexus-bootstrap --test ipc_schema_emit`).
    let StorageVectorQueryArgs {
        namespace,
        embedding,
        limit,
    } = parse_args(args, "vector_query")?;
    let limit = limit
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(DEFAULT_QUERY_LIMIT);
    let matches = engine
        .vector_query(&namespace, &embedding, limit)
        .map_err(|e| exec_err(format!("vector_query: {e}")))?;
    to_value(&matches, "vector_query")
}

pub(crate) fn delete_by_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageVectorDeleteArgs { namespace, path } = parse_args(args, "vector_delete_by_file")?;
    engine
        .vector_delete_by_file(&namespace, &path)
        .map_err(|e| exec_err(format!("vector_delete_by_file: {e}")))?;
    to_value(&StorageOk { ok: true }, "vector_delete_by_file")
}

pub(crate) fn count(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageVectorCountArgs { namespace } = parse_args(args, "vectorstore_count")?;
    let count = engine
        .vectorstore_count(&namespace)
        .map_err(|e| exec_err(format!("vectorstore_count: {e}")))?;
    // usize → u64 only ever truncates on hypothetical 128-bit hosts;
    // saturate rather than panic.
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    to_value(
        &StorageVectorstoreCountResult { count },
        "vectorstore_count",
    )
}

/// Project a typed `StorageChunkEmbedding` into the internal
/// `vectorstore::ChunkEmbedding` impl type. The wire shapes match
/// field-for-field; the conversion is a structural move.
fn chunk_to_impl(c: StorageChunkEmbedding) -> crate::vectorstore::ChunkEmbedding {
    crate::vectorstore::ChunkEmbedding {
        file_path: c.file_path,
        block_id: c.block_id,
        chunk_text: c.chunk_text,
        embedding: c.embedding,
        content_hash: c.content_hash,
    }
}

/// C19 (#372) — `vector_stored_signature`. Read-only lookup a caller
/// (`rag::index_file`) uses to decide whether a file's content has
/// changed since it was last embedded, without paying the cost of a
/// re-embed when it hasn't.
pub(crate) fn stored_signature(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let crate::ipc::StorageVectorStoredSignatureArgs {
        namespace,
        file_path,
    } = parse_args(args, "vector_stored_signature")?;
    let signature = engine
        .vector_stored_signature(&namespace, &file_path)
        .map_err(|e| exec_err(format!("vector_stored_signature: {e}")))?;
    let (content_hash, embedding_dim) = match signature {
        Some((hash, dim)) => (
            Some(hash),
            Some(u32::try_from(dim).unwrap_or(u32::MAX)),
        ),
        None => (None, None),
    };
    to_value(
        &crate::ipc::StorageVectorStoredSignatureResult {
            content_hash,
            embedding_dim,
        },
        "vector_stored_signature",
    )
}
