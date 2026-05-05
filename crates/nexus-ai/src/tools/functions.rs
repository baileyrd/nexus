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
//! Plus the read-only "extended" set registered by
//! [`register_extended_builtins`]:
//!
//! - [`SearchForgeTool`] — `search_forge` → `com.nexus.storage::search`.
//! - [`ListBacklinksTool`] — `list_backlinks` → `com.nexus.storage::backlinks`.
//! - [`GitLogTool`] — `git_log` → `com.nexus.git::log`.
//!
//! `terminal_exec` and `database_query` are still deferred — they
//! want their own capability surface (`process.spawn` for terminal,
//! etc.) that doesn't exist yet.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{KernelPluginContext, PluginContext};
use serde::{Deserialize, Serialize};

use super::registry::{ToolExecutor, ToolError, ToolRegistry, ToolSchema};

/// Plugin id of the storage core plugin — identical to the constant in
/// `vectorstore.rs` but kept local so this module is self-contained.
const STORAGE_PLUGIN: &str = "com.nexus.storage";

/// Plugin id of the git core plugin.
const GIT_PLUGIN: &str = "com.nexus.git";

/// Timeout applied to nested storage `ipc_call`s. File reads / writes
/// are local disk + index ops; 30s is an extreme upper bound matching
/// the existing `vectorstore` budget.
const STORAGE_IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Cap the model can request from `search_forge` / `git_log` so a
/// runaway tool call can't dump the whole index/history into the
/// prompt. 25 is enough for navigation; the model can refine the
/// query if it needs more.
const TOOL_RESULT_HARD_CAP: usize = 25;

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

// -- Extended read-only built-ins (G4) ---------------------------------
//
// Each tool below is a thin proxy: the schema describes the contract,
// the executor decodes the input, calls the target plugin's IPC
// handler, and renders the response back to the model. None of them
// mutate state — broadening the AI surface to read-only KG/VCS lookups
// before tackling write tools (which need their own capability work).

/// Schema for `search_forge` — full-text search across the forge.
#[must_use]
pub fn search_forge_schema() -> ToolSchema {
    ToolSchema {
        name: "search_forge".to_string(),
        description: "Full-text search across the forge. Returns matching \
                      blocks with their file paths and a relevance score. \
                      Use this to locate notes by content when you don't \
                      know the path. Limit defaults to 10, hard-capped at 25."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Tantivy-syntax full-text query."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 25,
                    "description": "Maximum results to return (default 10)."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    }
}

/// Schema for `list_backlinks` — incoming wikilinks to a file.
#[must_use]
pub fn list_backlinks_schema() -> ToolSchema {
    ToolSchema {
        name: "list_backlinks".to_string(),
        description: "List the files that link TO the given path via \
                      wikilinks (`[[…]]`). Useful for discovering \
                      which notes reference a topic. Returns an array \
                      of backlink records."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Forge-relative path of the target file."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    }
}

/// Schema for `git_log` — recent commits in the forge repo.
#[must_use]
pub fn git_log_schema() -> ToolSchema {
    ToolSchema {
        name: "git_log".to_string(),
        description: "Return recent commits from the forge's git history \
                      (most recent first). Returns an array with hash, \
                      author, ISO-8601 date, and message per commit. \
                      Errors cleanly when the forge isn't a git repo. \
                      Limit defaults to 20, hard-capped at 25."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 25,
                    "description": "Maximum commits to return (default 20)."
                }
            },
            "additionalProperties": false
        }),
    }
}

#[derive(Debug, Deserialize)]
struct SearchForgeArgs {
    query: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ListBacklinksArgs {
    path: String,
}

#[derive(Debug, Deserialize, Default)]
struct GitLogArgs {
    #[serde(default)]
    limit: Option<u32>,
}

/// `search_forge` executor — proxies to `com.nexus.storage::search`.
pub struct SearchForgeTool {
    ctx: Arc<KernelPluginContext>,
}

impl SearchForgeTool {
    /// Construct a `search_forge` tool bound to the AI plugin's kernel context.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl ToolExecutor for SearchForgeTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: SearchForgeArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("search_forge: {e}")))?;
        let limit = clamp_limit(args.limit, 10);

        let response = self
            .ctx
            .ipc_call(
                STORAGE_PLUGIN,
                "search",
                serde_json::json!({ "query": args.query, "limit": limit }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("storage search: {e}")))?;

        // Storage returns an array of hits; pass through as JSON. The
        // model does better with structured data here than with prose.
        Ok(response.to_string())
    }
}

/// `list_backlinks` executor — proxies to `com.nexus.storage::backlinks`.
pub struct ListBacklinksTool {
    ctx: Arc<KernelPluginContext>,
}

impl ListBacklinksTool {
    /// Construct a `list_backlinks` tool bound to the AI plugin's kernel context.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl ToolExecutor for ListBacklinksTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: ListBacklinksArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("list_backlinks: {e}")))?;

        let response = self
            .ctx
            .ipc_call(
                STORAGE_PLUGIN,
                "backlinks",
                serde_json::json!({ "path": args.path }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("storage backlinks: {e}")))?;

        Ok(response.to_string())
    }
}

/// `git_log` executor — proxies to `com.nexus.git::log`.
pub struct GitLogTool {
    ctx: Arc<KernelPluginContext>,
}

impl GitLogTool {
    /// Construct a `git_log` tool bound to the AI plugin's kernel context.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl ToolExecutor for GitLogTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: GitLogArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("git_log: {e}")))?;
        let limit = clamp_limit(args.limit, 20);

        let response = self
            .ctx
            .ipc_call(
                GIT_PLUGIN,
                "log",
                serde_json::json!({ "limit": limit }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("git log: {e}")))?;

        Ok(response.to_string())
    }
}

/// Clamp a caller-supplied `limit` into `[1, TOOL_RESULT_HARD_CAP]`,
/// substituting `default` when missing. The schema already declares
/// the bounds; this is a defence-in-depth check at the executor.
fn clamp_limit(requested: Option<u32>, default: usize) -> usize {
    let n = requested.map_or(default, |v| usize::try_from(v).unwrap_or(default));
    n.clamp(1, TOOL_RESULT_HARD_CAP)
}

/// Register the read-only extended built-ins (`search_forge`,
/// `list_backlinks`, `git_log`) onto an existing registry. Called
/// alongside [`register_storage_builtins`] from `wire_context`.
pub fn register_extended_builtins(registry: &mut ToolRegistry, ctx: Arc<KernelPluginContext>) {
    registry.register(
        "search_forge",
        search_forge_schema(),
        Arc::new(SearchForgeTool::new(Arc::clone(&ctx))),
    );
    registry.register(
        "list_backlinks",
        list_backlinks_schema(),
        Arc::new(ListBacklinksTool::new(Arc::clone(&ctx))),
    );
    registry.register(
        "git_log",
        git_log_schema(),
        Arc::new(GitLogTool::new(ctx)),
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

    #[test]
    fn search_forge_schema_requires_query() {
        let schema = search_forge_schema();
        assert_eq!(schema.name, "search_forge");
        let required: Vec<&str> = schema
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(required, ["query"]);
    }

    #[test]
    fn list_backlinks_schema_requires_path() {
        let schema = list_backlinks_schema();
        assert_eq!(schema.name, "list_backlinks");
        let required = schema
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required array");
        assert!(required.iter().any(|v| v == "path"));
    }

    #[test]
    fn git_log_schema_has_no_required_args() {
        let schema = git_log_schema();
        assert_eq!(schema.name, "git_log");
        // `required` is optional in JSON Schema; absent or empty is fine.
        let required = schema
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        assert_eq!(required, 0);
    }

    #[test]
    fn search_forge_args_reject_missing_query() {
        let err = serde_json::from_value::<SearchForgeArgs>(serde_json::json!({}))
            .expect_err("must fail");
        assert!(err.to_string().contains("query"));
    }

    #[test]
    fn clamp_limit_uses_default_when_missing() {
        assert_eq!(clamp_limit(None, 10), 10);
    }

    #[test]
    fn clamp_limit_caps_at_hard_max() {
        assert_eq!(clamp_limit(Some(9999), 10), TOOL_RESULT_HARD_CAP);
    }

    #[test]
    fn clamp_limit_floor_is_one() {
        assert_eq!(clamp_limit(Some(0), 10), 1);
    }

    /// Smoke-test that the extended built-ins all land in the registry
    /// under the names this audit promised. Registry round-trip is
    /// covered by `tools::registry::tests`; we just want to know we
    /// wired the names right.
    #[test]
    fn extended_builtins_register_under_documented_names() {
        use crate::tools::registry::ToolRegistry;
        use nexus_kernel::{
            CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore,
        };

        let dir = tempfile::tempdir().unwrap();
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let bus = Arc::new(EventBus::new(16));
        let caps: CapabilitySet = [nexus_kernel::Capability::IpcCall].into_iter().collect();
        let ctx = Arc::new(
            KernelPluginContext::new("com.nexus.ai", "0.0.1", caps, kv, bus, dir.path(), None)
                .unwrap(),
        );

        let mut registry = ToolRegistry::new();
        register_extended_builtins(&mut registry, ctx);

        let schemas = registry.schemas();
        let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
        for expected in ["search_forge", "list_backlinks", "git_log"] {
            assert!(
                names.contains(&expected),
                "{expected} missing from registry; saw {names:?}"
            );
        }
    }
}
