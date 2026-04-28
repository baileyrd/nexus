//! MCP server implementation: 13 tools for note CRUD, search, graph, tasks, and RAG.
//!
//! All tools route through the kernel plugin IPC boundary — the server holds
//! an `Arc<KernelPluginContext>` and issues `ipc_call`s to `com.nexus.storage`
//! and `com.nexus.ai`, so every tool call is capability-checked and auditable
//! at the kernel. `nexus_ask` dispatches to the AI plugin's `ask` handler
//! (RAG over indexed notes).

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{KernelPluginContext, PluginContext};
use rmcp::RoleServer;
use rmcp::ServiceExt as _;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    Annotated, CallToolRequestParams, CallToolResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, RawResource, ReadResourceRequestParams, ReadResourceResult, Resource,
    ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::schemars;
use rmcp::service::RequestContext;
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};

const STORAGE_PLUGIN: &str = "com.nexus.storage";
const AI_PLUGIN: &str = "com.nexus.ai";
const IPC_TIMEOUT: Duration = Duration::from_secs(30);
/// Longer timeout for AI calls — they make outbound HTTP requests to the
/// chat + embedding providers.
const AI_IPC_TIMEOUT: Duration = Duration::from_secs(120);

/// URI prefix for MCP resources representing forge notes (PRD-14 §7.1/§7.2).
///
/// Each note is exposed as `mcp://nexus/notes/<vault-relative-path>`. The
/// listing root (`mcp://nexus/notes`) is not itself a readable resource.
const NOTE_URI_PREFIX: &str = "mcp://nexus/notes/";

/// Parse the vault-relative path out of a `mcp://nexus/notes/...` URI.
///
/// Returns `None` for URIs that don't start with [`NOTE_URI_PREFIX`] and for
/// the bare notes root (`mcp://nexus/notes`) which has no path component.
pub(crate) fn parse_note_uri(uri: &str) -> Option<&str> {
    let rest = uri.strip_prefix(NOTE_URI_PREFIX)?;
    if rest.is_empty() { None } else { Some(rest) }
}

/// Build an MCP [`Resource`] descriptor for a forge note at `path`.
///
/// `size_bytes` is clamped to `u32::MAX` (the rmcp `RawResource::size` field
/// is `u32`); we use `try_from` rather than `as` to avoid silent truncation.
pub(crate) fn build_note_resource(path: &str, size_bytes: u64) -> Resource {
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string();
    Annotated::new(
        RawResource::new(format!("{NOTE_URI_PREFIX}{path}"), file_name)
            .with_description("Markdown note in the Nexus forge")
            .with_mime_type("text/markdown")
            .with_size(u32::try_from(size_bytes).unwrap_or(u32::MAX)),
        None,
    )
}

// ── Input types ──────────────────────────────────────────────────────────────

/// Input for the `nexus_read_note` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReadNoteInput {
    /// Vault-relative path to the note (e.g. "notes/hello.md").
    path: String,
}

/// Input for the `nexus_create_note` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateNoteInput {
    /// Vault-relative path for the new note.
    path: String,
    /// Markdown content of the note.
    content: String,
}

/// Input for the `nexus_update_note` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateNoteInput {
    /// Vault-relative path of the note to update.
    path: String,
    /// New markdown content for the note.
    content: String,
}

/// Input for the `nexus_delete_note` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeleteNoteInput {
    /// Vault-relative path of the note to delete.
    path: String,
}

/// Input for the `nexus_list_notes` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ListNotesInput {
    /// Optional path prefix to filter notes (e.g. "notes/projects/").
    prefix: Option<String>,
}

/// Input for the `nexus_search` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchInput {
    /// Search query string.
    query: String,
    /// Maximum number of results to return (default: 20).
    limit: Option<usize>,
}

/// Input for the `nexus_backlinks` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BacklinksInput {
    /// Vault-relative path of the note to find backlinks for.
    path: String,
}

/// Input for the `nexus_outgoing_links` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OutgoingLinksInput {
    /// Vault-relative path of the note to find outgoing links for.
    path: String,
}

/// Input for `nexus_graph_status` (no parameters).
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct GraphStatusInput {}

/// Input for `nexus_list_tags`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListTagsInput {
    /// Tag name (without the `#` prefix).
    name: String,
}

/// Input for `nexus_list_tasks`.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ListTasksInput {
    /// Filter by completion state; `None` returns both.
    completed: Option<bool>,
    /// Restrict to a specific file path.
    file: Option<String>,
}

/// Input for `nexus_toggle_task`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ToggleTaskInput {
    /// The task's database ID.
    task_id: u64,
}

/// Input for the `nexus_ask` RAG tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AskInput {
    /// The question to answer via RAG over the knowledge base.
    question: String,
}

// ── Output types ─────────────────────────────────────────────────────────────

/// Output for reading a note.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ReadNoteOutput {
    path: String,
    content: String,
    size_bytes: u64,
}

/// Output for creating/updating a note.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct WriteNoteOutput {
    path: String,
    size_bytes: u64,
    content_hash: String,
}

/// Output for deleting a note.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DeleteNoteOutput {
    deleted: bool,
}

/// A single file entry in a list response.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct FileEntry {
    path: String,
    size_bytes: u64,
    modified_at: i64,
}

/// Output for listing notes.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ListNotesOutput {
    count: usize,
    files: Vec<FileEntry>,
}

/// A single search hit.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SearchHit {
    file_path: String,
    block_type: String,
    excerpt: String,
    score: f32,
}

/// Output for search.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SearchOutput {
    count: usize,
    results: Vec<SearchHit>,
}

/// A single backlink entry.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct BacklinkEntry {
    source_path: String,
    link_text: String,
}

/// Output for backlinks.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct BacklinksOutput {
    count: usize,
    backlinks: Vec<BacklinkEntry>,
}

/// A single outgoing link entry.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct OutgoingLinkEntry {
    target_path: String,
    link_text: String,
    link_type: String,
    is_resolved: bool,
}

/// Output for outgoing links.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct OutgoingLinksOutput {
    count: usize,
    links: Vec<OutgoingLinkEntry>,
}

/// Output for graph status.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[allow(clippy::struct_field_names)]
struct GraphStatusOutput {
    node_count: usize,
    edge_count: usize,
    unresolved_count: usize,
}

/// A single tag entry.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct TagEntry {
    name: String,
    file_path: String,
    source: String,
}

/// Output for list tags.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ListTagsOutput {
    count: usize,
    tags: Vec<TagEntry>,
}

/// A single task entry.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct TaskEntry {
    id: u64,
    file_path: String,
    content: String,
    completed: bool,
    line_number: u32,
}

/// Output for list tasks.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ListTasksOutput {
    count: usize,
    tasks: Vec<TaskEntry>,
}

/// Output for toggle task.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ToggleTaskOutput {
    id: u64,
    file_path: String,
    content: String,
    completed: bool,
}

/// Output for the ask (RAG) tool.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct AskOutput {
    answer: String,
    model: String,
    source_count: usize,
}

// ── Server ───────────────────────────────────────────────────────────────────

/// MCP server that exposes Nexus forge operations as tools.
///
/// Holds an [`Arc<KernelPluginContext>`] and dispatches every tool call
/// through `context.ipc_call("com.nexus.storage", …)`.
pub struct NexusMcpServer {
    context: Arc<KernelPluginContext>,
    tool_router: ToolRouter<Self>,
}

impl NexusMcpServer {
    /// Create a new MCP server backed by the given plugin context.
    #[must_use]
    pub fn new(context: Arc<KernelPluginContext>) -> Self {
        Self {
            context,
            tool_router: Self::tool_router(),
        }
    }

    /// Start the server on stdio transport and block until disconnected.
    ///
    /// # Errors
    /// Returns an error if the transport or server fails to start.
    pub async fn serve_stdio(self) -> Result<(), Box<dyn std::error::Error>> {
        let transport = rmcp::transport::io::stdio();
        let server: rmcp::service::RunningService<RoleServer, Self> = self.serve(transport).await?;
        server.waiting().await?;
        Ok(())
    }

    async fn storage_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(STORAGE_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }
}

// ── Tool implementations ─────────────────────────────────────────────────────

#[tool_router]
impl NexusMcpServer {
    #[tool(
        name = "nexus_read_note",
        description = "Read a note's content by vault-relative path"
    )]
    async fn read_note(
        &self,
        Parameters(input): Parameters<ReadNoteInput>,
    ) -> Json<ReadNoteOutput> {
        #[derive(Deserialize)]
        struct Resp {
            bytes: Vec<u8>,
        }
        match self
            .storage_call::<Resp>("read_file", serde_json::json!({ "path": &input.path }))
            .await
        {
            Ok(r) => {
                let content = String::from_utf8_lossy(&r.bytes).into_owned();
                let size_bytes = r.bytes.len() as u64;
                Json(ReadNoteOutput {
                    path: input.path,
                    content,
                    size_bytes,
                })
            }
            Err(e) => Json(ReadNoteOutput {
                path: input.path,
                content: format!("Error: {e}"),
                size_bytes: 0,
            }),
        }
    }

    #[tool(
        name = "nexus_create_note",
        description = "Create a new note with the given path and markdown content"
    )]
    async fn create_note(
        &self,
        Parameters(input): Parameters<CreateNoteInput>,
    ) -> Json<WriteNoteOutput> {
        self.do_write_file(&input.path, &input.content).await
    }

    #[tool(
        name = "nexus_update_note",
        description = "Update an existing note's content (creates if it does not exist)"
    )]
    async fn update_note(
        &self,
        Parameters(input): Parameters<UpdateNoteInput>,
    ) -> Json<WriteNoteOutput> {
        self.do_write_file(&input.path, &input.content).await
    }

    #[tool(
        name = "nexus_delete_note",
        description = "Delete a note by vault-relative path"
    )]
    async fn delete_note(
        &self,
        Parameters(input): Parameters<DeleteNoteInput>,
    ) -> Json<DeleteNoteOutput> {
        match self
            .storage_call::<serde_json::Value>(
                "delete_file",
                serde_json::json!({ "path": &input.path }),
            )
            .await
        {
            Ok(_) => Json(DeleteNoteOutput { deleted: true }),
            Err(e) => {
                tracing::error!("delete_note failed for {}: {e}", input.path);
                Json(DeleteNoteOutput { deleted: false })
            }
        }
    }

    #[tool(
        name = "nexus_list_notes",
        description = "List notes in the forge, optionally filtered by a path prefix"
    )]
    async fn list_notes(
        &self,
        Parameters(input): Parameters<ListNotesInput>,
    ) -> Json<ListNotesOutput> {
        #[derive(Deserialize)]
        struct Rec {
            path: String,
            size_bytes: u64,
            #[serde(default)]
            modified_at: i64,
        }
        let prefix = input.prefix.as_deref().unwrap_or("");
        let args = if prefix.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({ "prefix": prefix })
        };
        match self.storage_call::<Vec<Rec>>("query_files", args).await {
            Ok(records) => {
                let files: Vec<FileEntry> = records
                    .into_iter()
                    .map(|r| FileEntry {
                        path: r.path,
                        size_bytes: r.size_bytes,
                        modified_at: r.modified_at,
                    })
                    .collect();
                Json(ListNotesOutput {
                    count: files.len(),
                    files,
                })
            }
            Err(e) => {
                tracing::error!("list_notes failed: {e}");
                Json(ListNotesOutput {
                    count: 0,
                    files: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_search",
        description = "Full-text search across notes. Rebuilds the search index before querying."
    )]
    async fn search_notes(&self, Parameters(input): Parameters<SearchInput>) -> Json<SearchOutput> {
        #[derive(Deserialize)]
        struct Hit {
            file_path: String,
            block_type: String,
            excerpt: String,
            score: f32,
        }
        if let Err(e) = self
            .storage_call::<serde_json::Value>("rebuild_search_index", serde_json::json!({}))
            .await
        {
            tracing::warn!("Failed to rebuild search index: {e}");
        }
        let limit = input.limit.unwrap_or(20);
        match self
            .storage_call::<Vec<Hit>>(
                "search",
                serde_json::json!({ "query": &input.query, "limit": limit }),
            )
            .await
        {
            Ok(hits) => {
                let results: Vec<SearchHit> = hits
                    .into_iter()
                    .map(|h| SearchHit {
                        file_path: h.file_path,
                        block_type: h.block_type,
                        excerpt: h.excerpt,
                        score: h.score,
                    })
                    .collect();
                Json(SearchOutput {
                    count: results.len(),
                    results,
                })
            }
            Err(e) => {
                tracing::error!("search failed: {e}");
                Json(SearchOutput {
                    count: 0,
                    results: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_backlinks",
        description = "Find all notes that link to the specified note (backlinks)"
    )]
    async fn backlinks(
        &self,
        Parameters(input): Parameters<BacklinksInput>,
    ) -> Json<BacklinksOutput> {
        #[derive(Deserialize)]
        struct Bl {
            source_path: String,
            link_text: String,
        }
        match self
            .storage_call::<Vec<Bl>>("backlinks", serde_json::json!({ "path": &input.path }))
            .await
        {
            Ok(bls) => {
                let backlinks: Vec<BacklinkEntry> = bls
                    .into_iter()
                    .map(|b| BacklinkEntry {
                        source_path: b.source_path,
                        link_text: b.link_text,
                    })
                    .collect();
                Json(BacklinksOutput {
                    count: backlinks.len(),
                    backlinks,
                })
            }
            Err(e) => {
                tracing::error!("backlinks failed: {e}");
                Json(BacklinksOutput {
                    count: 0,
                    backlinks: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_outgoing_links",
        description = "Find all outgoing links from the specified note"
    )]
    async fn outgoing_links(
        &self,
        Parameters(input): Parameters<OutgoingLinksInput>,
    ) -> Json<OutgoingLinksOutput> {
        // Fields match the JSON shape returned by storage's `outgoing_links`.
        #[derive(Deserialize)]
        #[allow(clippy::struct_field_names)]
        struct Link {
            target_path: String,
            link_text: String,
            link_type: String,
            is_resolved: bool,
        }
        match self
            .storage_call::<Vec<Link>>("outgoing_links", serde_json::json!({ "path": &input.path }))
            .await
        {
            Ok(ls) => {
                let links: Vec<OutgoingLinkEntry> = ls
                    .into_iter()
                    .map(|l| OutgoingLinkEntry {
                        target_path: l.target_path,
                        link_text: l.link_text,
                        link_type: l.link_type,
                        is_resolved: l.is_resolved,
                    })
                    .collect();
                Json(OutgoingLinksOutput {
                    count: links.len(),
                    links,
                })
            }
            Err(e) => {
                tracing::error!("outgoing_links failed: {e}");
                Json(OutgoingLinksOutput {
                    count: 0,
                    links: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_graph_status",
        description = "Get knowledge graph statistics: node count, edge count, unresolved links"
    )]
    async fn graph_status(
        &self,
        Parameters(_input): Parameters<GraphStatusInput>,
    ) -> Json<GraphStatusOutput> {
        // Fields match the JSON shape returned by storage's `graph_stats`.
        #[derive(Deserialize)]
        #[allow(clippy::struct_field_names)]
        struct Stats {
            node_count: usize,
            edge_count: usize,
            unresolved_count: usize,
        }
        match self
            .storage_call::<Stats>("graph_stats", serde_json::json!({}))
            .await
        {
            Ok(s) => Json(GraphStatusOutput {
                node_count: s.node_count,
                edge_count: s.edge_count,
                unresolved_count: s.unresolved_count,
            }),
            Err(e) => {
                tracing::error!("graph_status failed: {e}");
                Json(GraphStatusOutput {
                    node_count: 0,
                    edge_count: 0,
                    unresolved_count: 0,
                })
            }
        }
    }

    #[tool(
        name = "nexus_list_tags",
        description = "List all occurrences of a tag by name across the forge"
    )]
    async fn list_tags(
        &self,
        Parameters(input): Parameters<ListTagsInput>,
    ) -> Json<ListTagsOutput> {
        #[derive(Deserialize)]
        struct Tag {
            name: String,
            file_path: String,
            source: String,
        }
        match self
            .storage_call::<Vec<Tag>>("query_tags", serde_json::json!({ "name": &input.name }))
            .await
        {
            Ok(tags) => {
                let entries: Vec<TagEntry> = tags
                    .into_iter()
                    .map(|t| TagEntry {
                        name: t.name,
                        file_path: t.file_path,
                        source: t.source,
                    })
                    .collect();
                Json(ListTagsOutput {
                    count: entries.len(),
                    tags: entries,
                })
            }
            Err(e) => {
                tracing::error!("list_tags failed: {e}");
                Json(ListTagsOutput {
                    count: 0,
                    tags: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_list_tasks",
        description = "List tasks (checkboxes) across notes with optional completed/file filters"
    )]
    async fn list_tasks(
        &self,
        Parameters(input): Parameters<ListTasksInput>,
    ) -> Json<ListTasksOutput> {
        #[derive(Deserialize)]
        struct Task {
            id: u64,
            file_path: String,
            content: String,
            completed: bool,
            line_number: u32,
        }
        let args = serde_json::json!({
            "completed": input.completed,
            "file_path": input.file,
        });
        match self.storage_call::<Vec<Task>>("query_tasks", args).await {
            Ok(tasks) => {
                let entries: Vec<TaskEntry> = tasks
                    .into_iter()
                    .map(|t| TaskEntry {
                        id: t.id,
                        file_path: t.file_path,
                        content: t.content,
                        completed: t.completed,
                        line_number: t.line_number,
                    })
                    .collect();
                Json(ListTasksOutput {
                    count: entries.len(),
                    tasks: entries,
                })
            }
            Err(e) => {
                tracing::error!("list_tasks failed: {e}");
                Json(ListTasksOutput {
                    count: 0,
                    tasks: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_toggle_task",
        description = "Toggle a task's completed/incomplete state by its database ID"
    )]
    async fn toggle_task(
        &self,
        Parameters(input): Parameters<ToggleTaskInput>,
    ) -> Json<ToggleTaskOutput> {
        #[derive(Deserialize)]
        struct Rec {
            id: u64,
            file_path: String,
            content: String,
            completed: bool,
        }
        match self
            .storage_call::<Rec>(
                "toggle_task",
                serde_json::json!({ "task_id": input.task_id }),
            )
            .await
        {
            Ok(r) => Json(ToggleTaskOutput {
                id: r.id,
                file_path: r.file_path,
                content: r.content,
                completed: r.completed,
            }),
            Err(e) => Json(ToggleTaskOutput {
                id: input.task_id,
                file_path: String::new(),
                content: format!("Error: {e}"),
                completed: false,
            }),
        }
    }

    #[tool(
        name = "nexus_ask",
        description = "Ask a question via RAG over your notes"
    )]
    async fn ask(&self, Parameters(input): Parameters<AskInput>) -> Json<AskOutput> {
        #[derive(Deserialize)]
        struct Resp {
            answer: String,
            #[serde(default)]
            model: String,
            #[serde(default)]
            sources: Vec<serde_json::Value>,
        }
        let args = serde_json::json!({ "question": input.question, "limit": 5 });
        let value = match self
            .context
            .ipc_call(AI_PLUGIN, "ask", args, AI_IPC_TIMEOUT)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                return Json(AskOutput {
                    answer: format!("nexus_ask failed: {e}"),
                    model: String::new(),
                    source_count: 0,
                });
            }
        };
        match serde_json::from_value::<Resp>(value) {
            Ok(r) => Json(AskOutput {
                answer: r.answer,
                model: r.model,
                source_count: r.sources.len(),
            }),
            Err(e) => Json(AskOutput {
                answer: format!("nexus_ask: failed to decode response: {e}"),
                model: String::new(),
                source_count: 0,
            }),
        }
    }

    /// Shared `write_file` implementation for `create_note` + `update_note`.
    async fn do_write_file(&self, path: &str, content: &str) -> Json<WriteNoteOutput> {
        #[derive(Deserialize)]
        struct Meta {
            path: String,
            size_bytes: u64,
            content_hash: String,
        }
        match self
            .storage_call::<Meta>(
                "write_file",
                serde_json::json!({ "path": path, "bytes": content.as_bytes() }),
            )
            .await
        {
            Ok(m) => Json(WriteNoteOutput {
                path: m.path,
                size_bytes: m.size_bytes,
                content_hash: m.content_hash,
            }),
            Err(e) => Json(WriteNoteOutput {
                path: path.to_string(),
                size_bytes: 0,
                content_hash: format!("Error: {e}"),
            }),
        }
    }
}

// ── ServerHandler implementation ─────────────────────────────────────────────

impl rmcp::ServerHandler for NexusMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();
        info.with_instructions(
            "Nexus MCP server: manage a personal knowledge base of markdown notes. \
             Use nexus_* tools to create, read, update, delete, search, and query notes. \
             Forge notes are also enumerated as MCP resources under mcp://nexus/notes/.",
        )
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc)
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        let items = self.tool_router.list_all();
        std::future::ready(Ok(ListToolsResult {
            tools: items,
            ..Default::default()
        }))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, rmcp::ErrorData> {
        // Same `query_files` shape as nexus_list_notes (server.rs ~390): the
        // storage handler returns Vec<{ path, size_bytes, modified_at }>.
        #[derive(Deserialize)]
        struct Rec {
            path: String,
            size_bytes: u64,
        }
        let records: Vec<Rec> = self
            .storage_call("query_files", serde_json::json!({}))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(format!("query_files: {e}"), None))?;
        let resources: Vec<Resource> = records
            .into_iter()
            .map(|r| build_note_resource(&r.path, r.size_bytes))
            .collect();
        Ok(ListResourcesResult {
            resources,
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, rmcp::ErrorData> {
        #[derive(Deserialize)]
        struct ReadFileResp {
            bytes: Vec<u8>,
        }
        let Some(path) = parse_note_uri(&request.uri) else {
            return Err(rmcp::ErrorData::resource_not_found(
                format!("unknown resource uri: {}", request.uri),
                None,
            ));
        };
        let resp: ReadFileResp = self
            .storage_call("read_file", serde_json::json!({ "path": path }))
            .await
            .map_err(|e| {
                rmcp::ErrorData::resource_not_found(
                    format!("resource not found: {} ({e})", request.uri),
                    None,
                )
            })?;
        let text = String::from_utf8_lossy(&resp.bytes).into_owned();
        let contents = ResourceContents::text(text, &request.uri).with_mime_type("text/markdown");
        Ok(ReadResourceResult::new(vec![contents]))
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_note_uri_extracts_path() {
        assert_eq!(
            parse_note_uri("mcp://nexus/notes/foo/bar.md"),
            Some("foo/bar.md")
        );
        assert_eq!(parse_note_uri("file:///x"), None);
        // Notes root with no trailing path component.
        assert_eq!(parse_note_uri("mcp://nexus/notes"), None);
    }

    #[test]
    fn build_note_resource_sets_uri_mime_and_size() {
        let r = build_note_resource("foo.md", 123);
        assert_eq!(r.raw.uri, "mcp://nexus/notes/foo.md");
        assert_eq!(r.raw.mime_type.as_deref(), Some("text/markdown"));
        assert_eq!(r.raw.size, Some(123));
        assert_eq!(r.raw.name, "foo.md");
    }

    #[test]
    fn build_note_resource_clamps_oversize_to_u32_max() {
        let r = build_note_resource("huge.md", u64::MAX);
        assert_eq!(r.raw.size, Some(u32::MAX));
    }
}
