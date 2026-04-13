//! MCP server implementation with 13 tools for note CRUD, search, graph, tasks, and RAG.

use std::sync::Mutex;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ListToolsResult, ServerInfo,
};
use rmcp::schemars;
use rmcp::service::RequestContext;
use rmcp::ServiceExt as _;
use rmcp::RoleServer;
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};

use nexus_storage::StorageEngine;

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
    /// Search query string. Supports scope operators: tag:NAME, path:PREFIX.
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

/// Input for the `nexus_graph_status` tool (no parameters).
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct GraphStatusInput {}

/// Input for the `nexus_list_tags` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListTagsInput {
    /// Tag name to search for (without the # prefix).
    name: String,
}

/// Input for the `nexus_list_tasks` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ListTasksInput {
    /// Filter by completion state. None returns all tasks.
    completed: Option<bool>,
    /// Filter by file path. None returns tasks from all files.
    file: Option<String>,
}

/// Input for the `nexus_toggle_task` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ToggleTaskInput {
    /// Database ID of the task to toggle.
    task_id: u64,
}

/// Input for the `nexus_ask` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AskInput {
    /// Question to answer using RAG over the knowledge base.
    question: String,
}

// ── Output types ─────────────────────────────────────────────────────────────

/// Output for `read_note`: contains the file content and size.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ReadNoteOutput {
    path: String,
    content: String,
    size_bytes: u64,
}

/// Output for create/update note operations.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct WriteNoteOutput {
    path: String,
    size_bytes: u64,
    content_hash: String,
}

/// Output for `delete_note`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DeleteNoteOutput {
    deleted: bool,
}

/// A single file entry in list output.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct FileEntry {
    path: String,
    size_bytes: u64,
    modified_at: i64,
}

/// Output for `list_notes`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ListNotesOutput {
    count: usize,
    files: Vec<FileEntry>,
}

/// A single search hit.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SearchHit {
    file_path: String,
    block_id: u64,
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
    link_type: String,
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
/// Holds a [`StorageEngine`] behind a [`Mutex`] (required because
/// `StorageEngine` is `Send` but not `Sync`) and a tool router generated
/// by the `#[tool_router]` macro.
pub struct NexusMcpServer {
    storage: Mutex<StorageEngine>,
    tool_router: ToolRouter<Self>,
}

impl NexusMcpServer {
    /// Create a new MCP server backed by the given storage engine.
    #[must_use]
    pub fn new(storage: StorageEngine) -> Self {
        Self {
            storage: Mutex::new(storage),
            tool_router: Self::tool_router(),
        }
    }

    /// Start the server on stdio transport and block until disconnected.
    ///
    /// # Errors
    ///
    /// Returns an error if the transport or server fails to start.
    pub async fn serve_stdio(self) -> Result<(), Box<dyn std::error::Error>> {
        let transport = rmcp::transport::io::stdio();
        let server: rmcp::service::RunningService<RoleServer, Self> =
            self.serve(transport).await?;
        server.waiting().await?;
        Ok(())
    }
}

// ── Tool implementations ─────────────────────────────────────────────────────

#[tool_router]
impl NexusMcpServer {
    /// Read a note from the forge and return its content.
    #[tool(name = "nexus_read_note", description = "Read a note's content by vault-relative path")]
    fn read_note(&self, Parameters(input): Parameters<ReadNoteInput>) -> Json<ReadNoteOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.read_file(&input.path) {
            Ok(bytes) => {
                let content = String::from_utf8_lossy(&bytes).into_owned();
                let size_bytes = bytes.len() as u64;
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

    /// Create a new note in the forge.
    #[tool(name = "nexus_create_note", description = "Create a new note with the given path and markdown content")]
    fn create_note(&self, Parameters(input): Parameters<CreateNoteInput>) -> Json<WriteNoteOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.write_file(&input.path, input.content.as_bytes()) {
            Ok(meta) => Json(WriteNoteOutput {
                path: meta.path,
                size_bytes: meta.size_bytes,
                content_hash: meta.content_hash,
            }),
            Err(e) => Json(WriteNoteOutput {
                path: input.path,
                size_bytes: 0,
                content_hash: format!("Error: {e}"),
            }),
        }
    }

    /// Update an existing note (upsert semantics).
    #[tool(name = "nexus_update_note", description = "Update an existing note's content (creates if it does not exist)")]
    fn update_note(&self, Parameters(input): Parameters<UpdateNoteInput>) -> Json<WriteNoteOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.write_file(&input.path, input.content.as_bytes()) {
            Ok(meta) => Json(WriteNoteOutput {
                path: meta.path,
                size_bytes: meta.size_bytes,
                content_hash: meta.content_hash,
            }),
            Err(e) => Json(WriteNoteOutput {
                path: input.path,
                size_bytes: 0,
                content_hash: format!("Error: {e}"),
            }),
        }
    }

    /// Delete a note from the forge.
    #[tool(name = "nexus_delete_note", description = "Delete a note by vault-relative path")]
    fn delete_note(&self, Parameters(input): Parameters<DeleteNoteInput>) -> Json<DeleteNoteOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.delete_file(&input.path) {
            Ok(()) => Json(DeleteNoteOutput { deleted: true }),
            Err(e) => {
                tracing::error!("delete_note failed for {}: {e}", input.path);
                Json(DeleteNoteOutput { deleted: false })
            }
        }
    }

    /// List notes in the forge, optionally filtered by path prefix.
    #[tool(name = "nexus_list_notes", description = "List notes in the forge, optionally filtered by a path prefix")]
    fn list_notes(&self, Parameters(input): Parameters<ListNotesInput>) -> Json<ListNotesOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        let prefix = input.prefix.as_deref().unwrap_or("");
        match storage.list_files(prefix) {
            Ok(files) => {
                let entries: Vec<FileEntry> = files
                    .into_iter()
                    .map(|f| FileEntry {
                        path: f.path,
                        size_bytes: f.size_bytes,
                        modified_at: f.modified_at,
                    })
                    .collect();
                let count = entries.len();
                Json(ListNotesOutput {
                    count,
                    files: entries,
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

    /// Full-text search across all indexed notes.
    #[tool(
        name = "nexus_search",
        description = "Full-text search across notes. Supports scope operators: tag:NAME, path:PREFIX, prop:KEY:VALUE"
    )]
    fn search_notes(&self, Parameters(input): Parameters<SearchInput>) -> Json<SearchOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        let limit = input.limit.unwrap_or(20);

        // Rebuild the search index before querying to ensure fresh results.
        if let Err(e) = storage.rebuild_search_index() {
            tracing::warn!("Failed to rebuild search index: {e}");
        }

        match storage.search(&input.query, limit) {
            Ok(results) => {
                let hits: Vec<SearchHit> = results
                    .into_iter()
                    .map(|r| SearchHit {
                        file_path: r.file_path,
                        block_id: r.block_id,
                        block_type: r.block_type,
                        excerpt: r.excerpt,
                        score: r.score,
                    })
                    .collect();
                let count = hits.len();
                Json(SearchOutput {
                    count,
                    results: hits,
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

    /// Find all notes that link to a given note (backlinks).
    #[tool(name = "nexus_backlinks", description = "Find all notes that link to the specified note (backlinks)")]
    fn backlinks(&self, Parameters(input): Parameters<BacklinksInput>) -> Json<BacklinksOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.backlinks(&input.path) {
            Ok(links) => {
                let entries: Vec<BacklinkEntry> = links
                    .into_iter()
                    .map(|b| BacklinkEntry {
                        source_path: b.source_path,
                        link_text: b.link_text,
                        link_type: b.link_type,
                    })
                    .collect();
                let count = entries.len();
                Json(BacklinksOutput {
                    count,
                    backlinks: entries,
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

    /// Find all outgoing links from a given note.
    #[tool(name = "nexus_outgoing_links", description = "Find all outgoing links from the specified note")]
    fn outgoing_links(
        &self,
        Parameters(input): Parameters<OutgoingLinksInput>,
    ) -> Json<OutgoingLinksOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.outgoing_links(&input.path) {
            Ok(links) => {
                let entries: Vec<OutgoingLinkEntry> = links
                    .into_iter()
                    .map(|l| OutgoingLinkEntry {
                        target_path: l.target_path,
                        link_text: l.link_text,
                        link_type: l.link_type,
                        is_resolved: l.is_resolved,
                    })
                    .collect();
                let count = entries.len();
                Json(OutgoingLinksOutput {
                    count,
                    links: entries,
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

    /// Get knowledge graph statistics.
    #[tool(name = "nexus_graph_status", description = "Get knowledge graph statistics: node count, edge count, unresolved links")]
    fn graph_status(
        &self,
        Parameters(_input): Parameters<GraphStatusInput>,
    ) -> Json<GraphStatusOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.graph_stats() {
            Ok(stats) => Json(GraphStatusOutput {
                node_count: stats.node_count,
                edge_count: stats.edge_count,
                unresolved_count: stats.unresolved_count,
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

    /// List tags matching a given name.
    #[tool(name = "nexus_list_tags", description = "List all occurrences of a tag by name across the forge")]
    fn list_tags(&self, Parameters(input): Parameters<ListTagsInput>) -> Json<ListTagsOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.query_tags(&input.name) {
            Ok(tags) => {
                let entries: Vec<TagEntry> = tags
                    .into_iter()
                    .map(|t| TagEntry {
                        name: t.name,
                        file_path: t.file_path,
                        source: t.source,
                    })
                    .collect();
                let count = entries.len();
                Json(ListTagsOutput {
                    count,
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

    /// List tasks with optional filters.
    #[tool(
        name = "nexus_list_tasks",
        description = "List tasks (checkboxes) across notes with optional completed/file filters"
    )]
    fn list_tasks(&self, Parameters(input): Parameters<ListTasksInput>) -> Json<ListTasksOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        let filter = nexus_storage::TaskFilter {
            completed: input.completed,
            file_path: input.file,
        };
        match storage.query_tasks(&filter) {
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
                let count = entries.len();
                Json(ListTasksOutput {
                    count,
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

    /// Toggle a task's completion state.
    #[tool(name = "nexus_toggle_task", description = "Toggle a task's completed/incomplete state by its database ID")]
    fn toggle_task(
        &self,
        Parameters(input): Parameters<ToggleTaskInput>,
    ) -> Json<ToggleTaskOutput> {
        let storage = self.storage.lock().expect("storage mutex poisoned");
        match storage.toggle_task(input.task_id) {
            Ok(record) => Json(ToggleTaskOutput {
                id: record.id,
                file_path: record.file_path,
                content: record.content,
                completed: record.completed,
            }),
            Err(e) => Json(ToggleTaskOutput {
                id: input.task_id,
                file_path: String::new(),
                content: format!("Error: {e}"),
                completed: false,
            }),
        }
    }

    /// Ask a question using RAG over the knowledge base.
    #[tool(
        name = "nexus_ask",
        description = "Ask a question answered via RAG (retrieval-augmented generation) over your notes"
    )]
    async fn ask(&self, Parameters(input): Parameters<AskInput>) -> Json<AskOutput> {
        // Detect AI providers from environment.
        let Some(chat_config) = nexus_ai::detect_provider() else {
            return Json(AskOutput {
                answer: "No AI provider configured. Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or OLLAMA_BASE_URL.".into(),
                model: String::new(),
                source_count: 0,
            });
        };
        let Some(embed_config) = nexus_ai::detect_embedding_provider() else {
            return Json(AskOutput {
                answer: "No embedding provider configured. Set OPENAI_API_KEY or OLLAMA_BASE_URL.".into(),
                model: String::new(),
                source_count: 0,
            });
        };

        // Build providers.
        let ai: Box<dyn nexus_ai::AiProvider> = match chat_config.provider.as_str() {
            "anthropic" => Box::new(nexus_ai::AnthropicProvider::new(
                chat_config.api_key.unwrap_or_default(),
                chat_config.model,
                chat_config.max_tokens,
            )),
            "openai" => Box::new(nexus_ai::OpenAiProvider::new(
                chat_config.api_key.unwrap_or_default(),
                chat_config.model,
                chat_config.max_tokens,
            )),
            "ollama" => Box::new(nexus_ai::OllamaProvider::new(
                chat_config.base_url,
                chat_config.model,
            )),
            other => {
                return Json(AskOutput {
                    answer: format!("Unknown AI provider: {other}"),
                    model: String::new(),
                    source_count: 0,
                });
            }
        };

        let embedder: Box<dyn nexus_ai::EmbeddingProvider> = match embed_config.provider.as_str() {
            "openai" => Box::new(nexus_ai::OpenAiProvider::new(
                embed_config.api_key.unwrap_or_default(),
                embed_config.model,
                embed_config.max_tokens,
            )),
            "ollama" => Box::new(nexus_ai::OllamaProvider::new(
                embed_config.base_url,
                embed_config.model,
            )),
            other => {
                return Json(AskOutput {
                    answer: format!("Unknown embedding provider: {other}"),
                    model: String::new(),
                    source_count: 0,
                });
            }
        };

        // Get a DB connection for the vector store. Lock the storage briefly,
        // obtain a pooled connection, then drop the lock before awaiting.
        let conn = {
            let storage = self.storage.lock().expect("storage mutex poisoned");
            match storage.pool_connection() {
                Ok(c) => c,
                Err(e) => {
                    return Json(AskOutput {
                        answer: format!("Database error: {e}"),
                        model: String::new(),
                        source_count: 0,
                    });
                }
            }
        };

        // The PooledConnection derefs to &Connection. We need to run the async
        // RAG query with it. Since rusqlite::Connection is not Send, we use
        // tokio::task::block_in_place to run the async work on the current
        // thread without moving the connection across threads.
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(nexus_ai::rag_query(
                &conn,
                ai.as_ref(),
                embedder.as_ref(),
                &input.question,
                5,
            ))
        });

        match result {
            Ok(response) => Json(AskOutput {
                answer: response.answer,
                model: response.model,
                source_count: response.sources.len(),
            }),
            Err(e) => Json(AskOutput {
                answer: format!("RAG query failed: {e}"),
                model: String::new(),
                source_count: 0,
            }),
        }
    }
}

// ── ServerHandler implementation ─────────────────────────────────────────────

impl rmcp::ServerHandler for NexusMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
            .with_instructions(
                "Nexus MCP server: manage a personal knowledge base of markdown notes. \
                 Use nexus_* tools to create, read, update, delete, search, and query notes.",
            )
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_ {
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc)
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        let items = self.tool_router.list_all();
        std::future::ready(Ok(ListToolsResult {
            tools: items,
            ..Default::default()
        }))
    }
}
