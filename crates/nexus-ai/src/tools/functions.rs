//! Built-in [`ToolExecutor`] implementations the AI plugin can offer
//! out of the box.
//!
//! Per CLAUDE.md invariant 3 (IPC over direct calls), these executors
//! reach the rest of the runtime through `KernelPluginContext::ipc_call`.
//! They do **not** depend on `nexus-storage` or `nexus-editor` directly;
//! the cost is one extra capability check (`Capability::IpcCall`, already
//! held by `com.nexus.ai`) per invocation.
//!
//! This file ships the storage-backed pair the BL-016 spec calls out:
//!
//! - [`ReadFileTool`] — `read_file` → `com.nexus.storage::read_file`.
//! - [`WriteFileTool`] — `write_file` → `com.nexus.storage::write_file`.
//!
//! Other built-ins from PRD-12 §8.2 (`terminal_exec`, `database_query`,
//! `search`) are deferred — they want their own capability surface
//! (`process.spawn` for terminal, etc.) and don't fit the sub-task 1
//! scope.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{KernelPluginContext, PluginContext};
use serde::{Deserialize, Serialize};

use super::registry::{ToolExecutor, ToolError, ToolRegistry, ToolSchema};

/// Plugin id of the storage core plugin — identical to the constant in
/// `vectorstore.rs` but kept local so this module is self-contained.
const STORAGE_PLUGIN: &str = "com.nexus.storage";

/// Timeout applied to nested storage `ipc_call`s. File reads / writes
/// are local disk + index ops; 30s is an extreme upper bound matching
/// the existing `vectorstore` budget.
const STORAGE_IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Schema for the `read_file` built-in. Surfaced to the model so it
/// knows the tool exists and what argument shape to produce.
#[must_use]
pub fn read_file_schema() -> ToolSchema {
    ToolSchema {
        name: "read_file".to_string(),
        description: "Read the UTF-8 contents of a file in the forge. \
                      Use forge-relative paths (e.g. \"notes/today.md\"); \
                      absolute paths are rejected. Returns the file's \
                      text content. Errors if the file does not exist."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Forge-relative path to the file."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    }
}

/// Schema for the `write_file` built-in.
#[must_use]
pub fn write_file_schema() -> ToolSchema {
    ToolSchema {
        name: "write_file".to_string(),
        description: "Write or overwrite a file in the forge. The \
                      `content` is written as UTF-8 bytes; the storage \
                      layer updates its index and emits the usual file \
                      events. Returns a confirmation including the \
                      number of bytes written."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Forge-relative path to the file to write."
                },
                "content": {
                    "type": "string",
                    "description": "UTF-8 text to write to the file."
                }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }),
    }
}

/// Argument shape for [`ReadFileTool`].
#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    path: String,
}

/// Reply shape from `com.nexus.storage::read_file`.
#[derive(Debug, Deserialize)]
struct StorageReadReply {
    /// `None` when the file doesn't exist; `Some(bytes)` otherwise.
    bytes: Option<Vec<u8>>,
}

/// Argument shape for [`WriteFileTool`].
#[derive(Debug, Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

/// Read-only file tool: dispatches to `com.nexus.storage::read_file`
/// and decodes the reply as UTF-8.
pub struct ReadFileTool {
    ctx: Arc<KernelPluginContext>,
}

impl ReadFileTool {
    /// Construct a read-file tool bound to the AI plugin's kernel
    /// context. The caller must ensure `com.nexus.ai` holds the
    /// `ipc.call` capability or every dispatch will surface a clear
    /// [`ToolError::ExecutionFailed`] to the model.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl ToolExecutor for ReadFileTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: ReadFileArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("read_file: {e}")))?;

        let response = self
            .ctx
            .ipc_call(
                STORAGE_PLUGIN,
                "read_file",
                serde_json::json!({ "path": args.path }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("storage read_file: {e}")))?;

        let reply: StorageReadReply = serde_json::from_value(response)
            .map_err(|e| ToolError::ExecutionFailed(format!("read_file: decode: {e}")))?;

        let bytes = reply.bytes.ok_or_else(|| {
            ToolError::ExecutionFailed(format!("file not found: {}", args.path))
        })?;

        String::from_utf8(bytes)
            .map_err(|e| ToolError::ExecutionFailed(format!("read_file: not UTF-8: {e}")))
    }
}

/// Write tool: dispatches to `com.nexus.storage::write_file`. The
/// returned confirmation string includes the byte count so the model
/// can sanity-check what landed.
pub struct WriteFileTool {
    ctx: Arc<KernelPluginContext>,
}

impl WriteFileTool {
    /// Construct a write-file tool bound to the AI plugin's kernel
    /// context.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

/// Reply from `com.nexus.storage::write_file` (subset we care about).
/// The full `FileMetadata` carries `path`, `size_bytes`, `modified_at`,
/// `content_hash`; we surface enough back to the model that it knows
/// the write succeeded without dumping internal fields.
#[derive(Debug, Deserialize, Serialize)]
struct StorageWriteReply {
    path: String,
    #[serde(default)]
    size_bytes: Option<u64>,
}

#[async_trait]
impl ToolExecutor for WriteFileTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: WriteFileArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("write_file: {e}")))?;

        let bytes = args.content.as_bytes().to_vec();
        let byte_count = bytes.len();

        let response = self
            .ctx
            .ipc_call(
                STORAGE_PLUGIN,
                "write_file",
                serde_json::json!({ "path": args.path, "bytes": bytes }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("storage write_file: {e}")))?;

        // The storage handler returns `FileMetadata`. We only need the
        // path back; tolerate decode failure with a generic confirmation
        // since the write itself succeeded.
        let path = match serde_json::from_value::<StorageWriteReply>(response) {
            Ok(reply) => reply.path,
            Err(_) => args.path,
        };
        Ok(format!("Wrote {byte_count} bytes to {path}"))
    }
}

/// Register the storage-backed built-ins (`read_file`, `write_file`)
/// onto an existing registry. Convenience for `AiCorePlugin` and the
/// streaming dispatch loop that lands in sub-task 2.
pub fn register_storage_builtins(registry: &mut ToolRegistry, ctx: Arc<KernelPluginContext>) {
    registry.register(
        "read_file",
        read_file_schema(),
        Arc::new(ReadFileTool::new(Arc::clone(&ctx))),
    );
    registry.register(
        "write_file",
        write_file_schema(),
        Arc::new(WriteFileTool::new(ctx)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_schema_has_required_path() {
        let schema = read_file_schema();
        assert_eq!(schema.name, "read_file");
        let required = schema
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required array");
        assert!(required.iter().any(|v| v == "path"));
    }

    #[test]
    fn write_file_schema_requires_path_and_content() {
        let schema = write_file_schema();
        assert_eq!(schema.name, "write_file");
        let required: Vec<&str> = schema
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert!(required.contains(&"path"));
        assert!(required.contains(&"content"));
    }

    #[test]
    fn read_file_args_reject_missing_path() {
        let err =
            serde_json::from_value::<ReadFileArgs>(serde_json::json!({})).expect_err("must fail");
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn write_file_args_reject_missing_content() {
        let err = serde_json::from_value::<WriteFileArgs>(serde_json::json!({ "path": "x.md" }))
            .expect_err("must fail");
        assert!(err.to_string().contains("content"));
    }
}
