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
//! And the BL-055 terminal trio registered by
//! [`register_terminal_builtins`]:
//!
//! - [`TerminalRunSavedTool`] — `terminal_run_saved` →
//!   `com.nexus.terminal::run_saved`.
//! - [`TerminalGetStatusTool`] — `terminal_get_status` →
//!   `com.nexus.terminal::get_session_info`.
//! - [`TerminalSendSignalTool`] — `terminal_send_signal` →
//!   `com.nexus.terminal::send_raw_input` (with the right control
//!   byte for the requested terminal signal).
//!
//! `database_query` is still deferred — it wants its own capability
//! surface that doesn't exist yet.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nexus_kernel::{Ipc as _, KernelPluginContext};
use serde::{Deserialize, Serialize};

use super::registry::{ToolExecutor, ToolError, ToolRegistry, ToolSchema};

/// Plugin id of the storage core plugin — identical to the constant in
/// `vectorstore.rs` but kept local so this module is self-contained.
const STORAGE_PLUGIN: &str = "com.nexus.storage";

/// Plugin id of the git core plugin.
const GIT_PLUGIN: &str = "com.nexus.git";

/// Plugin id of the terminal core plugin (BL-055).
const TERMINAL_PLUGIN: &str = "com.nexus.terminal";

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

// -- BL-055 — terminal built-ins -------------------------------------
//
// Three thin proxies onto `com.nexus.terminal` that give an agent a
// minimal but complete handle on the process surface: start a saved
// command, ask whether a session is still running / what its exit
// code was, and shove a control byte (Ctrl-C / Ctrl-Z / Ctrl-D) at
// it. Anything more elaborate (managed lifecycle, log tailing) goes
// through subsequent tools rather than growing this set.
//
// `terminal_run_saved` and `terminal_send_signal` are write-class —
// they require `ai.tools.write` per ADR 0022 Phase 2. Only
// `terminal_get_status` is safe under `ai.chat` alone, so it's the
// lone terminal tool included in [`AutoReadOnly`].

/// Schema for `terminal_run_saved` — start a saved command in a fresh
/// session.
#[must_use]
pub fn terminal_run_saved_schema() -> ToolSchema {
    ToolSchema {
        name: "terminal_run_saved".to_string(),
        description: "Start a saved shell command in a new terminal \
                      session. The command runs as `<shell> -c \
                      \"<shell_cmd>\"` (or the equivalent one-shot flag \
                      for cmd.exe / pwsh) under the saved command's \
                      `working_dir` and `env_vars`. Returns the new \
                      session id; poll `terminal_get_status` to read \
                      exit status. Use `slug` to identify which saved \
                      command to launch — list via the user's saved \
                      commands sidebar."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "slug": {
                    "type": "string",
                    "description": "Slug (URL-safe id) of the saved command."
                },
                "working_dir": {
                    "type": "string",
                    "description": "Optional working-directory override; \
                                    falls back to the saved command's own \
                                    working_dir, or the inherited cwd."
                }
            },
            "required": ["slug"],
            "additionalProperties": false
        }),
    }
}

/// Schema for `terminal_get_status` — query a session's running /
/// exit state.
#[must_use]
pub fn terminal_get_status_schema() -> ToolSchema {
    ToolSchema {
        name: "terminal_get_status".to_string(),
        description: "Return metadata for a terminal session: whether \
                      the process is still running, its exit code (if \
                      any), and basic identifying fields. Read-only — \
                      safe to call repeatedly while polling for a \
                      build / test to finish."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Session id returned by `terminal_run_saved`."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        }),
    }
}

/// Schema for `terminal_send_signal` — send a control byte to a
/// session's PTY so the foreground process group receives the
/// corresponding terminal signal.
#[must_use]
pub fn terminal_send_signal_schema() -> ToolSchema {
    ToolSchema {
        name: "terminal_send_signal".to_string(),
        description: "Send a terminal control character to a running \
                      session so its foreground process group receives \
                      the matching signal. Supported values: `SIGINT` \
                      (Ctrl-C, ETX 0x03), `SIGQUIT` (Ctrl-\\, FS 0x1c), \
                      `SIGTSTP` (Ctrl-Z, SUB 0x1a), `EOF` (Ctrl-D, EOT \
                      0x04). For SIGTERM / SIGKILL of unresponsive \
                      processes, the user must close the session — no \
                      out-of-band signal path is exposed yet."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Session id returned by `terminal_run_saved`."
                },
                "signal": {
                    "type": "string",
                    "enum": ["SIGINT", "SIGQUIT", "SIGTSTP", "EOF"],
                    "description": "Which control character to send."
                }
            },
            "required": ["id", "signal"],
            "additionalProperties": false
        }),
    }
}

#[derive(Debug, Deserialize)]
struct TerminalRunSavedArgs {
    slug: String,
    #[serde(default)]
    working_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TerminalSessionIdArgs {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TerminalSendSignalArgs {
    id: String,
    signal: String,
}

/// `terminal_run_saved` — proxies to `com.nexus.terminal::run_saved`.
pub struct TerminalRunSavedTool {
    ctx: Arc<KernelPluginContext>,
}

impl TerminalRunSavedTool {
    /// Construct a `terminal_run_saved` tool bound to the AI plugin's
    /// kernel context.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl ToolExecutor for TerminalRunSavedTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: TerminalRunSavedArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("terminal_run_saved: {e}")))?;
        let mut payload = serde_json::json!({ "slug": args.slug });
        if let Some(wd) = args.working_dir {
            payload["working_dir"] = serde_json::Value::String(wd);
        }
        let response = self
            .ctx
            .ipc_call(
                TERMINAL_PLUGIN,
                "run_saved",
                payload,
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("terminal run_saved: {e}")))?;
        Ok(response.to_string())
    }
}

/// `terminal_get_status` — proxies to
/// `com.nexus.terminal::get_session_info`.
pub struct TerminalGetStatusTool {
    ctx: Arc<KernelPluginContext>,
}

impl TerminalGetStatusTool {
    /// Construct a `terminal_get_status` tool bound to the AI plugin's
    /// kernel context.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl ToolExecutor for TerminalGetStatusTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: TerminalSessionIdArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("terminal_get_status: {e}")))?;
        let response = self
            .ctx
            .ipc_call(
                TERMINAL_PLUGIN,
                "get_session_info",
                serde_json::json!({ "id": args.id }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("terminal get_session_info: {e}")))?;
        Ok(response.to_string())
    }
}

/// Map a logical signal name to the single byte that, when written
/// through the controlling PTY, produces the corresponding signal in
/// the foreground process group. Anything else returns
/// `InvalidInput` so the model can correct itself.
fn signal_byte(signal: &str) -> Result<u8, ToolError> {
    match signal {
        "SIGINT" => Ok(0x03),  // ETX (Ctrl-C)
        "SIGQUIT" => Ok(0x1c), // FS  (Ctrl-\)
        "SIGTSTP" => Ok(0x1a), // SUB (Ctrl-Z)
        "EOF" => Ok(0x04),     // EOT (Ctrl-D)
        other => Err(ToolError::InvalidInput(format!(
            "terminal_send_signal: unsupported signal '{other}'; expected SIGINT|SIGQUIT|SIGTSTP|EOF"
        ))),
    }
}

/// `terminal_send_signal` — proxies to
/// `com.nexus.terminal::send_raw_input` with the byte that drives the
/// requested terminal signal.
pub struct TerminalSendSignalTool {
    ctx: Arc<KernelPluginContext>,
}

impl TerminalSendSignalTool {
    /// Construct a `terminal_send_signal` tool bound to the AI plugin's
    /// kernel context.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl ToolExecutor for TerminalSendSignalTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let args: TerminalSendSignalArgs = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("terminal_send_signal: {e}")))?;
        let byte = signal_byte(&args.signal)?;
        self.ctx
            .ipc_call(
                TERMINAL_PLUGIN,
                "send_raw_input",
                serde_json::json!({ "id": args.id, "data": [byte] }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("terminal send_raw_input: {e}")))?;
        Ok(format!("Sent {} to {}", args.signal, args.id))
    }
}

/// Register the BL-055 terminal built-ins (`terminal_run_saved`,
/// `terminal_get_status`, `terminal_send_signal`) onto an existing
/// registry. Called alongside [`register_storage_builtins`] /
/// [`register_extended_builtins`] from `wire_context`.
pub fn register_terminal_builtins(registry: &mut ToolRegistry, ctx: Arc<KernelPluginContext>) {
    registry.register(
        "terminal_run_saved",
        terminal_run_saved_schema(),
        Arc::new(TerminalRunSavedTool::new(Arc::clone(&ctx))),
    );
    registry.register(
        "terminal_get_status",
        terminal_get_status_schema(),
        Arc::new(TerminalGetStatusTool::new(Arc::clone(&ctx))),
    );
    registry.register(
        "terminal_send_signal",
        terminal_send_signal_schema(),
        Arc::new(TerminalSendSignalTool::new(ctx)),
    );
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

    #[test]
    fn terminal_run_saved_schema_requires_slug() {
        let s = terminal_run_saved_schema();
        assert_eq!(s.name, "terminal_run_saved");
        let required: Vec<&str> = s
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(required, ["slug"]);
    }

    #[test]
    fn terminal_get_status_schema_requires_id() {
        let s = terminal_get_status_schema();
        assert_eq!(s.name, "terminal_get_status");
        let required: Vec<&str> = s
            .input_schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(required, ["id"]);
    }

    #[test]
    fn terminal_send_signal_schema_constrains_signal_enum() {
        let s = terminal_send_signal_schema();
        let signal = s.input_schema["properties"]["signal"]["enum"]
            .as_array()
            .expect("enum array");
        let names: Vec<&str> = signal.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(names, ["SIGINT", "SIGQUIT", "SIGTSTP", "EOF"]);
    }

    #[test]
    fn signal_byte_maps_known_signals() {
        assert_eq!(signal_byte("SIGINT").unwrap(), 0x03);
        assert_eq!(signal_byte("SIGQUIT").unwrap(), 0x1c);
        assert_eq!(signal_byte("SIGTSTP").unwrap(), 0x1a);
        assert_eq!(signal_byte("EOF").unwrap(), 0x04);
    }

    #[test]
    fn signal_byte_rejects_unknown_signal_with_invalid_input() {
        let err = signal_byte("SIGKILL").unwrap_err();
        match err {
            ToolError::InvalidInput(msg) => assert!(msg.contains("SIGKILL")),
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn terminal_builtins_register_under_documented_names() {
        use crate::tools::registry::ToolRegistry;
        use nexus_kernel::{CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore,
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
        register_terminal_builtins(&mut registry, ctx);
        let names: Vec<String> = registry.schemas().into_iter().map(|s| s.name).collect();
        for expected in [
            "terminal_run_saved",
            "terminal_get_status",
            "terminal_send_signal",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} missing; saw {names:?}"
            );
        }
    }

    /// Smoke-test that the extended built-ins all land in the registry
    /// under the names this audit promised. Registry round-trip is
    /// covered by `tools::registry::tests`; we just want to know we
    /// wired the names right.
    #[test]
    fn extended_builtins_register_under_documented_names() {
        use crate::tools::registry::ToolRegistry;
        use nexus_kernel::{CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore,
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
