//! Vector-store handlers: `vector_insert`, `vector_query`,
//! `vector_delete_by_file`, `vectorstore_count`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::StorageEngine;

use super::shared::{exec_err, path_arg, to_value};

pub(crate) fn insert(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let file_path = args
        .get("file_path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("vector_insert: missing 'file_path' string".to_string()))?
        .to_string();
    let chunks: Vec<crate::vectorstore::ChunkEmbedding> = args
        .get("chunks")
        .ok_or_else(|| exec_err("vector_insert: missing 'chunks'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("vector_insert: chunks decode: {e}")))
        })?;
    engine
        .vector_insert(&file_path, &chunks)
        .map_err(|e| exec_err(format!("vector_insert: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn query(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let embedding: Vec<f32> = args
        .get("embedding")
        .ok_or_else(|| exec_err("vector_query: missing 'embedding'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("vector_query: embedding decode: {e}")))
        })?;
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(5);
    let matches = engine
        .vector_query(&embedding, limit)
        .map_err(|e| exec_err(format!("vector_query: {e}")))?;
    to_value(&matches, "vector_query")
}

pub(crate) fn delete_by_file(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "vector_delete_by_file")?;
    engine
        .vector_delete_by_file(&path)
        .map_err(|e| exec_err(format!("vector_delete_by_file: {e}")))?;
    Ok(serde_json::json!({}))
}

pub(crate) fn count(engine: &StorageEngine) -> Result<Value, PluginError> {
    let count = engine
        .vectorstore_count()
        .map_err(|e| exec_err(format!("vectorstore_count: {e}")))?;
    Ok(serde_json::json!({ "count": count }))
}
