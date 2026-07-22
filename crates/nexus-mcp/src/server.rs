//! MCP server implementation: 15 tools for note CRUD, search, graph, tasks, RAG, and skills.
//!
//! All tools route through the kernel plugin IPC boundary — the server holds
//! an `Arc<KernelPluginContext>` and issues `ipc_call`s to `com.nexus.storage`,
//! `com.nexus.ai`, and `com.nexus.skills`, so every tool call is capability-checked
//! and auditable at the kernel. `nexus_ask` dispatches to the AI plugin's `ask`
//! handler (RAG over indexed notes); `nexus_list_skills` / `nexus_render_skill`
//! surface authored prompt templates from `.forge/skills/` so external clients
//! can invoke them as named, parameterised prompts.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nexus_kernel::{EventFilter, Events as _, Ipc as _, KernelPluginContext, NexusEvent};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    Annotated, CallToolRequestParams, CallToolResult, GetPromptRequestParams, GetPromptResult,
    ListPromptsResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt,
    PromptArgument, PromptMessage, PromptMessageRole, RawResource, ReadResourceRequestParams,
    ReadResourceResult, Resource, ResourceContents, ResourceUpdatedNotificationParam,
    ServerCapabilities, ServerInfo,
};
use rmcp::schemars;
use rmcp::service::{Peer, RequestContext};
use rmcp::RoleServer;
use rmcp::ServiceExt as _;
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};

use nexus_types::constants::{IPC_TIMEOUT_EXTENDED, IPC_TIMEOUT_LONG, IPC_TIMEOUT_SHORT};
use nexus_types::plugin_ids;

const STORAGE_PLUGIN: &str = plugin_ids::STORAGE;
const AI_PLUGIN: &str = plugin_ids::AI;
const SKILLS_PLUGIN: &str = plugin_ids::SKILLS;
/// BL-115 — `nexus_detect_changes` reaches into `com.nexus.git` for
/// the working-tree status, then joins against the BL-114
/// code-symbol index. Kept as a separate const so the plugin id is
/// reusable from a future `git_call` helper.
const GIT_PLUGIN: &str = plugin_ids::GIT;
/// BL-137 — `nexus_kernel_stats` reaches into `com.nexus.security` for
/// the live [`nexus_kernel::KernelMetrics`] snapshot, since the
/// security plugin's `metrics_snapshot` handler is the canonical
/// IPC surface for the global metrics registry. Read-only.
const SECURITY_PLUGIN: &str = plugin_ids::SECURITY;
/// Memory engine — the `nexus_memory_*` tools call its
/// `search`/`add`/`list`/`facts`/`entities` IPC handlers. No centralized
/// `plugin_ids` const yet, so the id is inline.
const MEMORY_PLUGIN: &str = "com.nexus.memory";
/// C66 (#419) — `nexus_export_html` reaches into `com.nexus.formats` for
/// the styled single-note HTML exporter's `export_html` handler.
const FORMATS_PLUGIN: &str = plugin_ids::FORMATS;
/// C74 (#427) — the `nexus_comment_*` tools reach into
/// `com.nexus.comments` for the 7 thread/comment CRUD handlers.
const COMMENTS_PLUGIN: &str = plugin_ids::COMMENTS;
/// C74 (#427) — `nexus_comment_create_thread` needs `com.nexus.editor`
/// to resolve a block-tree position into a stable `block_id` before
/// it can call `comments::create_thread` (the one handler in the
/// comments surface with no other anchor to hang a headless
/// create-thread call on).
const EDITOR_PLUGIN: &str = plugin_ids::EDITOR;
/// C75 (#428) — `nexus_agent_run`/`nexus_agent_sessions` reach into
/// `com.nexus.agent`'s `session_run`/`session_list` handlers so an
/// external MCP client can delegate a goal to an agent archetype.
const AGENT_PLUGIN: &str = plugin_ids::AGENT;
/// C75 (#428) — `nexus_workflow_list`/`nexus_workflow_run` reach into
/// `com.nexus.workflow`'s `list`/`run` handlers so an external MCP
/// client can fire a user-authored workflow.
const WORKFLOW_PLUGIN: &str = plugin_ids::WORKFLOW;
/// P2-06 — default deadline the MCP server applies to inbound IPC
/// calls into kernel-side plugins (storage, git, security, …).
/// Override via a future `[mcp.timeouts] ipc_secs = N` block
/// (deferred from P2-06).
pub const DEFAULT_IPC_TIMEOUT: Duration = IPC_TIMEOUT_SHORT;
const IPC_TIMEOUT: Duration = DEFAULT_IPC_TIMEOUT;
/// P2-06 — longer deadline for AI calls — they make outbound HTTP
/// requests to the chat + embedding providers.
pub const DEFAULT_AI_IPC_TIMEOUT: Duration = IPC_TIMEOUT_LONG;
const AI_IPC_TIMEOUT: Duration = DEFAULT_AI_IPC_TIMEOUT;
/// C75 (#428) — deadline for `nexus_agent_run` / `nexus_workflow_run`:
/// both can drive many LLM/tool round-trips end-to-end, so they need
/// the same extended budget the CLI's `agent run` / `workflow run`
/// use rather than the short default.
pub const DEFAULT_RUN_IPC_TIMEOUT: Duration = IPC_TIMEOUT_EXTENDED;
const RUN_IPC_TIMEOUT: Duration = DEFAULT_RUN_IPC_TIMEOUT;

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
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
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

/// `com.nexus.terminal` — RFC 0003 Track A. The `nexus_terminal_get_*` tools and
/// the `mcp://nexus/terminal/...` resources read the server-side VT grid.
const TERMINAL_PLUGIN: &str = plugin_ids::TERMINAL;

/// URI prefix for MCP resources representing a terminal session's VT grid state.
/// Each session exposes `mcp://nexus/terminal/<id>/<kind>` for the kinds below.
const TERMINAL_URI_PREFIX: &str = "mcp://nexus/terminal/";

/// The readable VT-grid resource kinds, with descriptions, per terminal session.
const TERMINAL_RESOURCE_KINDS: &[(&str, &str)] = &[
    ("screen", "Current visible screen, as text."),
    (
        "scrollback",
        "Lines that scrolled off the top, oldest first.",
    ),
    ("cwd", "Working directory reported by the child (OSC 7)."),
    ("cursor", "Cursor position as \"col,row\" (zero-based)."),
    ("exit", "Exit code of the last finished command (OSC 133)."),
    ("command", "Output of the last finished command (OSC 133)."),
];

/// Parse `(session_id, kind)` out of a `mcp://nexus/terminal/<id>/<kind>` URI.
/// Returns `None` for non-terminal URIs or a missing id/kind component.
pub(crate) fn parse_terminal_uri(uri: &str) -> Option<(&str, &str)> {
    let rest = uri.strip_prefix(TERMINAL_URI_PREFIX)?;
    let (id, kind) = rest.split_once('/')?;
    if id.is_empty() || kind.is_empty() {
        return None;
    }
    Some((id, kind))
}

/// Build an MCP [`Resource`] descriptor for one VT-grid `kind` of a session.
pub(crate) fn build_terminal_resource(id: &str, kind: &str, description: &str) -> Resource {
    Annotated::new(
        RawResource::new(
            format!("{TERMINAL_URI_PREFIX}{id}/{kind}"),
            format!("terminal {id} · {kind}"),
        )
        .with_description(description)
        .with_mime_type("text/plain"),
        None,
    )
}

/// What a terminal lifecycle event means for the resource notifier — split out
/// of the async loop so the routing is unit-testable without a live peer/bus.
enum NotifyAction {
    /// Push these resource kinds and (re)stamp the session's debounce slot.
    PushResources(&'static [&'static str]),
    /// Push the screen resource, subject to the output debounce window.
    ScreenDebounced,
    /// Session ended — drop its debounce slot.
    Release,
    /// Nothing the notifier reacts to.
    Ignore,
}

/// Map a terminal event `kind` (the serde tag of `TerminalEvent`) to the
/// notifier's reaction. `session_evicted` releases the slot alongside
/// `session_closed` so the debounce map can't leak across LRU evictions (L4).
fn classify_terminal_event(kind: &str) -> NotifyAction {
    match kind {
        "command_finished" => NotifyAction::PushResources(&["screen", "exit", "command"]),
        "output_received" => NotifyAction::ScreenDebounced,
        "session_closed" | "session_evicted" => NotifyAction::Release,
        _ => NotifyAction::Ignore,
    }
}

/// Whether a kernel-bus `recv` error should end the notifier loop. A `Lagged`
/// gap is recoverable — the slow consumer skips the dropped span and keeps
/// going; only a `Closed` bus is terminal. Collapsing both to "stop" killed
/// notifications permanently after a single lag (H1).
fn recv_error_is_terminal(err: &nexus_kernel::RecvError) -> bool {
    matches!(err, nexus_kernel::RecvError::Closed)
}

/// RFC 0003 Track A — bridge terminal lifecycle events on the kernel bus into MCP
/// `notifications/resources/updated`, so a subscribed client learns when a
/// session's screen / exit / command resources change without polling.
///
/// On `CommandFinished` (OSC 133;D) the screen + exit + command resources are
/// pushed; the chatty `OutputReceived` stream is debounced to at most one screen
/// push per session per `OUTPUT_DEBOUNCE`. Best-effort: a no-op when no tokio
/// runtime is in scope (e.g. a sync test harness), and it exits quietly when the
/// bus or the client peer closes.
fn spawn_terminal_resource_notifier(context: &KernelPluginContext, peer: Peer<RoleServer>) {
    /// Debounce window for screen pushes driven by the output byte stream.
    const OUTPUT_DEBOUNCE: Duration = Duration::from_millis(750);

    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let mut sub = context.subscribe(EventFilter::CustomPrefix(
        "com.nexus.terminal.events.".to_string(),
    ));
    handle.spawn(async move {
        let mut last_screen_push: HashMap<String, Instant> = HashMap::new();
        loop {
            let evt = match sub.recv().await {
                Ok(evt) => evt,
                // A slow consumer that falls behind the bus capacity gets a
                // Lagged error for the dropped span; that is recoverable — skip
                // the gap and keep notifying. Only a Closed bus ends the loop.
                // (Collapsing both to `break` killed notifications permanently
                // after a single lag — H1.)
                Err(e) => {
                    if recv_error_is_terminal(&e) {
                        break;
                    }
                    continue;
                }
            };
            let NexusEvent::Custom { payload, .. } = &evt.event else {
                continue;
            };
            let kind = payload
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let Some(id) = payload.get("id").and_then(serde_json::Value::as_str) else {
                continue;
            };
            match classify_terminal_event(kind) {
                // A command finished: screen, exit, and command resources changed.
                NotifyAction::PushResources(kinds) => {
                    for k in kinds {
                        notify_terminal_resource(&peer, id, k).await;
                    }
                    last_screen_push.insert(id.to_string(), Instant::now());
                }
                // New output: push a screen update, debounced (the stream is chatty).
                NotifyAction::ScreenDebounced => {
                    let now = Instant::now();
                    let due = last_screen_push
                        .get(id)
                        .is_none_or(|t| now.duration_since(*t) >= OUTPUT_DEBOUNCE);
                    if due {
                        notify_terminal_resource(&peer, id, "screen").await;
                        last_screen_push.insert(id.to_string(), now);
                    }
                }
                // Session ended (closed or LRU-evicted): release its debounce
                // slot so the map can't grow unbounded across many short-lived
                // sessions. Eviction was previously missed, leaking one entry per
                // evicted session — L4.
                NotifyAction::Release => {
                    last_screen_push.remove(id);
                }
                NotifyAction::Ignore => {}
            }
        }
    });
}

/// Push one `notifications/resources/updated` for a terminal VT-grid resource.
async fn notify_terminal_resource(peer: &Peer<RoleServer>, id: &str, kind: &str) {
    let uri = format!("{TERMINAL_URI_PREFIX}{id}/{kind}");
    let _ = peer
        .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri))
        .await;
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

/// Input for the single-session `nexus_terminal_get_*` tools (RFC 0003).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TerminalSessionInput {
    /// Terminal session id (from `list_sessions` / the terminal sidebar).
    session_id: String,
}

/// Input for the `nexus_terminal_get_scrollback` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TerminalScrollbackInput {
    /// Terminal session id.
    session_id: String,
    /// Max scrollback lines to return (most recent, oldest first). Default 1000.
    lines: Option<usize>,
}

/// Input for the `nexus_search` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchInput {
    /// Search query string.
    query: String,
    /// Maximum number of results to return (default: 20).
    limit: Option<usize>,
    /// #375 — skip this many ranked hits before taking the page of
    /// `limit`, for paging through results.
    #[serde(default)]
    offset: Option<usize>,
    /// #375 — sort order: `"relevance"` (default), `"mtime_desc"`, or
    /// `"mtime_asc"`.
    #[serde(default)]
    sort: Option<String>,
    /// #375 — only include blocks whose file mtime is on or after this
    /// Unix-seconds timestamp.
    #[serde(default)]
    mtime_after: Option<i64>,
    /// #375 — only include blocks whose file mtime is on or before
    /// this Unix-seconds timestamp.
    #[serde(default)]
    mtime_before: Option<i64>,
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

/// Input for `nexus_entity_get` (C48 / #401).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EntityGetInput {
    /// Canonical entity id or one of its aliases.
    id: String,
}

/// Input for `nexus_entity_search` (C48 / #401).
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct EntitySearchInput {
    /// Substring query against entity id / aliases / description.
    /// Empty string returns the lexicographically-first `limit`
    /// entities.
    #[serde(default)]
    query: String,
    /// Optional case-insensitive filter on `entity_type`.
    #[serde(default)]
    entity_type: Option<String>,
    /// Maximum hits to return. Defaults to 10 when omitted.
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `nexus_entity_relations` (C48 / #401).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EntityRelationsInput {
    /// Canonical entity id or alias.
    id: String,
    /// One of `"outgoing"` / `"incoming"` / `"both"`. Defaults to
    /// `"both"` when omitted.
    #[serde(default)]
    direction: Option<String>,
}

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

/// Input for `nexus_semantic_search` (C78 #431).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SemanticSearchInput {
    /// Query to embed and retrieve against — natural language, not
    /// keyword syntax.
    query: String,
    /// Maximum number of ranked chunks to return.
    #[serde(default = "default_semantic_search_limit")]
    limit: u32,
    /// Fuse the vector ranking with lexical Tantivy BM25 (RRF) instead
    /// of vector-only retrieval. Surfaces keyword-exact hits that
    /// embed poorly alongside semantic hits that share no keywords.
    #[serde(default)]
    hybrid: bool,
}

fn default_semantic_search_limit() -> u32 {
    10
}

/// Reply for `nexus_semantic_search`: raw ranked chunk hits, unlike
/// `nexus_ask` which synthesizes a chat answer. `matches` carries
/// `ChunkMatch` shape (`file_path`, `block_id`, `chunk_text`, `score`)
/// for vector-only retrieval, or the hybrid `StorageHybridMatch` shape
/// (`file_path`, `block_id`, `block_type`, `excerpt`, `score`,
/// `fts_rank`, `vector_rank`) when `hybrid: true`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SemanticSearchOutput {
    /// The AI plugin's `semantic_search` reply (or an `{ "error": … }`
    /// object on failure).
    result: serde_json::Value,
}

/// Input for `nexus_list_skills` (no parameters).
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct ListSkillsInput {}

/// Input for the `nexus_render_skill` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RenderSkillInput {
    /// The skill's id (matches the `id:` front-matter field).
    id: String,
    /// Optional values keyed by the skill's declared placeholder names.
    /// Omitted placeholders fall back to the skill's defaults.
    #[serde(default)]
    values: serde_json::Map<String, serde_json::Value>,
}

// ── BL-115 code-intel inputs ────────────────────────────────────────────────

/// Input for the `nexus_context` tool (BL-115).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NexusContextInput {
    /// Identifier as it appears in source. Case-sensitive.
    name: String,
    /// Optional forge-relative path to scope the lookup. A name that
    /// resolves to multiple files (e.g. a `new` method on two impls)
    /// returns every match unless `path` is set.
    #[serde(default)]
    path: Option<String>,
}

/// Input for the `nexus_impact` tool (BL-115).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[allow(
    dead_code,
    reason = "depth is in the JSON Schema for forward-compat but ignored in v1"
)]
struct NexusImpactInput {
    /// Identifier as it appears in source.
    name: String,
    /// Optional forge-relative path scope.
    #[serde(default)]
    path: Option<String>,
    /// Traversal depth. v1 honours `0` (symbol only) and `1`
    /// (direct neighbours — siblings + parent + path-mates).
    /// Higher values are accepted but treated as `1` since the
    /// BL-114 index has no call-edges to traverse; see the
    /// `degraded` flag on the reply.
    #[serde(default)]
    depth: Option<u32>,
}

/// Input for the `nexus_detect_changes` tool (BL-115). No
/// parameters — the tool always queries the active forge's working
/// tree.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct NexusDetectChangesInput {}

/// Input for the `nexus_kernel_stats` tool (BL-137). No parameters.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct NexusKernelStatsInput {}

// ── Output types ─────────────────────────────────────────────────────────────

/// BL-115 — explanation surfaced verbatim on every `degraded: true`
/// reply. Lets agents emit a "I used the BL-114 index which doesn't
/// have call edges" caveat without inventing the wording.
const BL115_DEGRADED_REASON: &str =
    "BL-114's code-symbol index records declarations only; call-edge \
     traversal lands in a follow-up BL. nexus_context/impact return \
     parent + sibling-method proxies for direct callers, and nexus_detect_changes \
     joins git statuses against indexed symbols rather than tracing diff hunks \
     into the AST.";

/// BL-115 — mirror of `nexus_storage::ipc::StorageSymbolRow`, kept
/// local so the MCP server doesn't have to depend on `nexus-storage`
/// for the type alone. Round-tripped from the wire-level reply.
#[derive(Debug, Deserialize)]
struct QuerySymbolRow {
    id: i64,
    path: String,
    language: String,
    kind: String,
    name: String,
    line_start: u32,
    line_end: u32,
    #[serde(default)]
    parent_id: Option<i64>,
    #[serde(default)]
    doc_comment: Option<String>,
}

/// Wrapper for the `query_symbol` reply envelope.
#[derive(Debug, Deserialize)]
struct QuerySymbolReply {
    symbols: Vec<QuerySymbolRow>,
}

/// BL-115 — kind → risk-band heuristic. v1 is intentionally
/// coarse-grained because the index has no call-edges to count
/// fan-out from; agents see the `degraded` flag and can ask for a
/// follow-up read with the source. Thresholds chosen so that:
///
/// - LOW = local in scope (methods, consts, statics)
/// - MEDIUM = top-level functions, structs/enums (data shape)
/// - HIGH = traits/interfaces (every implementor depends)
/// - CRITICAL = modules/impls/macros (containers of many other
///   symbols — a change here can cascade to every item inside)
fn risk_for_kind(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "method" | "const" | "static" => (
            "LOW",
            "scoped to its enclosing type; siblings on the same impl are the most likely callers",
        ),
        "function" => (
            "MEDIUM",
            "top-level function; could be called from anywhere in the crate",
        ),
        "struct" | "enum" | "union" | "class" | "type_alias" => (
            "MEDIUM",
            "data-shape symbol; every reader of the field set is affected by a layout change",
        ),
        "trait" | "interface" => (
            "HIGH",
            "every implementor depends on the trait contract; a signature change ripples",
        ),
        "macro" => (
            "HIGH",
            "macro callers expand at every use-site; output-shape changes can cascade",
        ),
        "module" | "impl" => (
            "CRITICAL",
            "container of many other symbols; a wholesale change here affects every item it owns",
        ),
        _ => (
            "MEDIUM",
            "unrecognised symbol kind; defaulting to MEDIUM until the kind is classified",
        ),
    }
}

fn clone_symbol_ref(r: &SymbolRef) -> SymbolRef {
    SymbolRef {
        id: r.id,
        name: r.name.clone(),
        kind: r.kind.clone(),
        line_start: r.line_start,
    }
}

// ── BL-115 code-intel outputs ───────────────────────────────────────────────

/// Compact reference to one indexed symbol. Used as a child / sibling
/// pointer in [`SymbolContext`] / [`ImpactReport`].
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SymbolRef {
    id: i64,
    name: String,
    kind: String,
    line_start: u32,
}

/// One resolved symbol with its enclosing context. The `siblings`
/// list collects symbols inside the same parent (e.g. every method
/// declared on the same `impl`).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SymbolContext {
    id: i64,
    path: String,
    name: String,
    kind: String,
    language: String,
    line_start: u32,
    line_end: u32,
    doc_comment: Option<String>,
    parent: Option<SymbolRef>,
    siblings: Vec<SymbolRef>,
}

/// Reply for `nexus_context`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct NexusContextOutput {
    matches: Vec<SymbolContext>,
    /// `true` whenever the index can't answer with full call-graph
    /// fidelity. v1 always sets this true because BL-114 ships
    /// symbol declarations only — no call-edge traversal. Agents
    /// can downweight confidence in their reasoning when this is
    /// set.
    degraded: bool,
    /// Human-readable note about what's missing — surfaced verbatim
    /// to MCP clients so the agent's prompt can carry the caveat.
    degraded_reason: Option<String>,
}

/// One symbol's blast-radius classification.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ImpactReport {
    symbol: SymbolContext,
    /// `"LOW"` / `"MEDIUM"` / `"HIGH"` / `"CRITICAL"` — matches the
    /// GitNexus rubric. v1 maps kinds to a fixed band; see
    /// `risk_for_kind` for thresholds.
    risk: String,
    /// One-line justification for the risk band.
    risk_reason: String,
    /// Symbols inside the same parent (siblings on the same impl /
    /// class). v1 surrogate for "direct callers" — a sibling method
    /// is the most likely caller without a real call graph.
    direct_affected: Vec<SymbolRef>,
}

/// Reply for `nexus_impact`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct NexusImpactOutput {
    matches: Vec<ImpactReport>,
    degraded: bool,
    degraded_reason: Option<String>,
}

/// Reply for `nexus_detect_changes`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct NexusDetectChangesOutput {
    /// Forge-relative paths reported dirty by git.
    changed_files: Vec<String>,
    /// Symbols whose containing file appears in `changed_files`.
    affected_symbols: Vec<SymbolRef>,
    /// Total `changed_files.len()` echoed for client convenience.
    total_dirty: usize,
    /// Same caveat as `nexus_context` / `nexus_impact`.
    degraded: bool,
    degraded_reason: Option<String>,
}

/// Input for `nexus_git_fetch` (C49 #402).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NexusGitFetchInput {
    /// Remote name (e.g. `"origin"`).
    remote: String,
}

/// Input for `nexus_git_pull` (C49 #402).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NexusGitPullInput {
    /// Remote name (e.g. `"origin"`).
    remote: String,
    /// Branch name to merge in (e.g. `"main"`).
    branch: String,
}

/// Input for `nexus_git_remotes` (C49 #402). No parameters.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct NexusGitRemotesInput {}

/// Reply for `nexus_git_fetch` / a generic ok-flag git op.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct GitOkOutput {
    /// The git plugin's reply (or an `{ "error": … }` object on failure).
    result: serde_json::Value,
}

// ── C74 (#427) — nexus_comment_* tools ───────────────────────────────────────

/// Input for `nexus_comment_list`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CommentListInput {
    /// Forge-relative path of the markdown file.
    file_path: String,
}

/// Input for `nexus_comment_create_thread`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CommentCreateThreadInput {
    /// Forge-relative path of the markdown file.
    file_path: String,
    /// Body text of the first comment in the new thread.
    body: String,
    /// 0-based index into the file's top-level blocks (as returned by
    /// `com.nexus.editor::get_tree`'s `tree.root_blocks`) to anchor
    /// the thread to. Defaults to `0` (the file's first block) — a
    /// headless caller with no editor selection has no other natural
    /// anchor to offer.
    #[serde(default)]
    block_index: Option<u32>,
    /// Optional author display name.
    #[serde(default)]
    author: Option<String>,
}

/// Input for `nexus_comment_add_reply`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CommentAddReplyInput {
    /// Forge-relative path of the markdown file.
    file_path: String,
    /// Thread to append to.
    thread_id: String,
    /// Reply body.
    body: String,
    /// Optional author display name.
    #[serde(default)]
    author: Option<String>,
}

/// Input for `nexus_comment_set_resolved`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CommentSetResolvedInput {
    /// Forge-relative path of the markdown file.
    file_path: String,
    /// Thread to mark.
    thread_id: String,
    /// New resolved flag.
    resolved: bool,
    /// Author of the resolution flip (best-effort).
    #[serde(default)]
    author: Option<String>,
}

/// Input for `nexus_comment_delete_thread`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CommentDeleteThreadInput {
    /// Forge-relative path of the markdown file.
    file_path: String,
    /// Thread to delete.
    thread_id: String,
}

/// Input for `nexus_comment_delete_comment`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CommentDeleteCommentInput {
    /// Forge-relative path of the markdown file.
    file_path: String,
    /// Thread containing the comment.
    thread_id: String,
    /// Comment to delete.
    comment_id: String,
}

/// Input for `nexus_comment_edit_comment`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CommentEditCommentInput {
    /// Forge-relative path of the markdown file.
    file_path: String,
    /// Thread containing the comment.
    thread_id: String,
    /// Comment to edit.
    comment_id: String,
    /// New body text.
    body: String,
}

/// Reply for every `nexus_comment_*` tool.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct CommentResultOutput {
    /// The comments plugin's reply (or an `{ "error": … }` object on failure).
    result: serde_json::Value,
}

// ── C75 (#428) — nexus_agent_* / nexus_workflow_* tools ──────────────────────

/// Input for `nexus_agent_run`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AgentRunInput {
    /// Natural-language goal for the agent to pursue.
    goal: String,
    /// Archetype — writer / coder / researcher / general (default).
    #[serde(default)]
    archetype: Option<String>,
}

/// Input for `nexus_agent_sessions`. No parameters.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct AgentSessionsInput {}

/// Reply for `nexus_agent_run` / `nexus_agent_sessions`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct AgentResultOutput {
    /// The agent plugin's reply (or an `{ "error": … }` object on failure).
    result: serde_json::Value,
}

/// Input for `nexus_workflow_list`. No parameters.
#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
struct WorkflowListInput {}

/// Input for `nexus_workflow_run`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WorkflowRunInput {
    /// Name of a loaded `.workflow.toml` to execute end-to-end.
    name: String,
}

/// Reply for `nexus_workflow_list` / `nexus_workflow_run`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct WorkflowResultOutput {
    /// The workflow plugin's reply (or an `{ "error": … }` object on failure).
    result: serde_json::Value,
}

/// Output for `nexus_kernel_stats` (BL-137). Mirrors the
/// `MetricsSnapshot` shape returned by
/// `com.nexus.security::metrics_snapshot`. Field names + types match
/// so an MCP client can reach for the same keys the shell health
/// panel uses. `null` when the snapshot is unavailable (kernel
/// metrics not installed — only happens in tests that don't boot a
/// full runtime).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct NexusKernelStatsOutput {
    /// `null` when `nexus_kernel::metrics::global()` is unset (no
    /// runtime up) — otherwise the snapshot blob mirrored verbatim
    /// from `com.nexus.security::metrics_snapshot`.
    snapshot: Option<serde_json::Value>,
}

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
    /// #375 — the block's file mtime, Unix seconds.
    mtime: i64,
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

/// One outgoing relation declared on an entity (C48 / #401). Mirrors
/// `nexus_storage::ipc::EntityRelationRow`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EntityRelationEntry {
    /// Target entity id or alias as declared on disk.
    target: String,
    /// Free-form relation kind.
    #[serde(rename = "type")]
    kind: String,
    /// Confidence in `[0.0, 1.0]`.
    confidence: f32,
}

/// Full entity payload for `nexus_entity_get`. Mirrors
/// `nexus_storage::ipc::EntityRecordRow`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EntityRecord {
    id: String,
    entity_type: String,
    aliases: Vec<String>,
    description: String,
    relations: Vec<EntityRelationEntry>,
    relpath: String,
}

/// Output for `nexus_entity_get`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EntityGetOutput {
    /// `None` when no id / alias matched.
    entity: Option<EntityRecord>,
}

/// One hit in `nexus_entity_search`'s results.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EntitySearchHit {
    id: String,
    entity_type: String,
    description: String,
    relpath: String,
    score: i32,
}

/// Output for `nexus_entity_search`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EntitySearchOutput {
    count: usize,
    results: Vec<EntitySearchHit>,
}

/// One row in `nexus_entity_relations`'s results. Aliased targets are
/// resolved to their canonical id.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EntityRelationsRow {
    from: String,
    to: String,
    #[serde(rename = "type")]
    kind: String,
    confidence: f32,
}

/// Output for `nexus_entity_relations`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct EntityRelationsOutput {
    count: usize,
    relations: Vec<EntityRelationsRow>,
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

/// A single skill entry in a list response.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SkillEntry {
    id: String,
    name: String,
    description: String,
    version: String,
    tags: Vec<String>,
    applicable_contexts: Vec<String>,
}

/// Output for `nexus_list_skills`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ListSkillsOutput {
    count: usize,
    skills: Vec<SkillEntry>,
}

/// Output for `nexus_render_skill`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct RenderSkillOutput {
    id: String,
    name: String,
    body: String,
}

// ── Dynamic tool helpers (DG-39) ─────────────────────────────────────────────

/// Convert a [`crate::dynamic_tools::DynamicTool`] declaration into
/// an rmcp `Tool` so it can be returned from `list_tools`. The schema
/// is wrapped in `Arc<JsonObject>` per the rmcp shape; if the
/// declaration's `input_schema` is not a JSON object we substitute
/// an empty object so the client still sees a valid schema.
fn dynamic_tool_to_rmcp(t: &crate::dynamic_tools::DynamicTool) -> rmcp::model::Tool {
    let schema_obj = match &t.input_schema {
        serde_json::Value::Object(o) => o.clone(),
        _ => serde_json::Map::new(),
    };
    // `rmcp::model::Tool` is `#[non_exhaustive]`, so we can't use a
    // struct literal. Mutate a default instead.
    let mut tool = rmcp::model::Tool::default();
    tool.name = std::borrow::Cow::Owned(t.name.clone());
    tool.description = Some(std::borrow::Cow::Owned(t.description.clone()));
    tool.input_schema = std::sync::Arc::new(schema_obj);
    tool
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
        // Clone the context before `serve` consumes `self`, so the terminal
        // resource-change notifier can subscribe to the kernel bus.
        let context = Arc::clone(&self.context);
        let server: rmcp::service::RunningService<RoleServer, Self> = self.serve(transport).await?;
        spawn_terminal_resource_notifier(&context, server.peer().clone());
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

    async fn skills_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(SKILLS_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// RFC 0003 Track A — `com.nexus.terminal` IPC client for the VT-grid
    /// introspection handlers (`get_screen` / `get_scrollback` / `get_cwd` /
    /// `get_cursor` / `get_last_exit`).
    async fn terminal_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(TERMINAL_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// Run a terminal introspection handler and return its JSON result, folding
    /// any error into `{ "error": ... }` so a tool always returns a body.
    async fn terminal_tool(&self, command: &str, args: serde_json::Value) -> serde_json::Value {
        match self.terminal_call::<serde_json::Value>(command, args).await {
            Ok(v) => v,
            Err(e) => serde_json::json!({ "error": e }),
        }
    }

    /// Read one VT-grid resource `kind` for session `id` as text, mapping the
    /// kind to its introspection handler (RFC 0003 Track A `read_resource`).
    async fn read_terminal_resource(&self, id: &str, kind: &str) -> Result<String, String> {
        let args = serde_json::json!({ "id": id });
        Ok(match kind {
            "screen" => {
                #[derive(Deserialize)]
                struct R {
                    text: String,
                }
                self.terminal_call::<R>("get_screen", args).await?.text
            }
            "scrollback" => {
                #[derive(Deserialize)]
                struct R {
                    text: String,
                }
                self.terminal_call::<R>("get_scrollback", args).await?.text
            }
            "cwd" => {
                #[derive(Deserialize)]
                struct R {
                    cwd: String,
                }
                self.terminal_call::<R>("get_cwd", args).await?.cwd
            }
            "cursor" => {
                #[derive(Deserialize)]
                struct R {
                    col: usize,
                    row: usize,
                }
                let r = self.terminal_call::<R>("get_cursor", args).await?;
                format!("{},{}", r.col, r.row)
            }
            "exit" => {
                #[derive(Deserialize)]
                struct R {
                    exit_code: Option<i32>,
                }
                self.terminal_call::<R>("get_last_exit", args)
                    .await?
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
            "command" => {
                #[derive(Deserialize)]
                struct R {
                    output: Option<String>,
                }
                self.terminal_call::<R>("get_last_exit", args)
                    .await?
                    .output
                    .unwrap_or_default()
            }
            other => return Err(format!("unknown terminal resource kind: {other}")),
        })
    }

    /// BL-115 — `com.nexus.git` IPC client, originally added for
    /// `nexus_detect_changes`; C49 (#402) reuses it for the
    /// `nexus_git_*` remote-sync tools.
    async fn git_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(GIT_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// BL-137 — `com.nexus.security` IPC client used by `nexus_kernel_stats`.
    async fn security_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(SECURITY_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// `com.nexus.memory` IPC client for the `nexus_memory_*` tools.
    async fn memory_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(MEMORY_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// `com.nexus.formats` IPC client for `nexus_export_html` (C66).
    async fn formats_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(FORMATS_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// C74 (#427) — `com.nexus.comments` IPC client for the
    /// `nexus_comment_*` tools.
    async fn comments_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(COMMENTS_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// C74 (#427) — `com.nexus.editor` IPC client used only by
    /// `nexus_comment_create_thread`'s block-id anchor resolution
    /// (`open` → `get_tree` → `stamp_block`).
    async fn editor_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(EDITOR_PLUGIN, command, args, IPC_TIMEOUT)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// C75 (#428) — `com.nexus.agent` IPC client for `nexus_agent_run` /
    /// `nexus_agent_sessions`. `nexus_agent_run` drives a full
    /// tool-calling session (possibly many LLM round-trips), so it
    /// gets the same extended deadline the CLI's `agent run` uses;
    /// `nexus_agent_sessions` is a plain read and uses the default.
    async fn agent_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(AGENT_PLUGIN, command, args, timeout)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }

    /// C75 (#428) — `com.nexus.workflow` IPC client for
    /// `nexus_workflow_list` / `nexus_workflow_run`. `run` executes
    /// every step of a user-authored workflow (may itself call AI or
    /// other IPC handlers) so it gets the same extended deadline
    /// `nexus_agent_run` and the CLI's `workflow run` use; `list` is
    /// a plain read and uses the default.
    async fn workflow_call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> Result<T, String> {
        let value = self
            .context
            .ipc_call(WORKFLOW_PLUGIN, command, args, timeout)
            .await
            .map_err(|e| format!("ipc {command}: {e}"))?;
        serde_json::from_value(value).map_err(|e| format!("decode {command}: {e}"))
    }
}

// ── Tool implementations ─────────────────────────────────────────────────────

/// Input for `nexus_memory_search`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemorySearchInput {
    /// Full-text query over captured/stored memories.
    query: String,
    /// Max results (default 20).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `nexus_memory_add`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemoryAddInput {
    /// The memory text to store.
    content: String,
    /// Optional category (default `general`).
    #[serde(default)]
    category: Option<String>,
    /// Optional tags.
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Optional cognitive type: `episodic` | `semantic` | `procedural` | `unclassified`.
    #[serde(default)]
    memory_type: Option<String>,
    /// Optional subject of an SPO entity fact (e.g. `ada`).
    #[serde(default)]
    subject: Option<String>,
    /// Optional predicate of an SPO entity fact (e.g. `writes`).
    #[serde(default)]
    predicate: Option<String>,
    /// Optional object of an SPO entity fact (e.g. `rust`).
    #[serde(default)]
    object: Option<String>,
}

/// Input for `nexus_memory_recent`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryRecentInput {
    /// Max results (default 50).
    #[serde(default)]
    limit: Option<u32>,
    /// Optional tag filter — only memories carrying this tag.
    #[serde(default)]
    tag: Option<String>,
}

/// Input for `nexus_memory_tags`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryTagsInput {
    /// Max tags to return (default 50).
    #[serde(default)]
    limit: Option<u32>,
}

/// A list of tags with memory counts.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemoryTagsOutput {
    /// Distinct tags as `{ key, count }` objects (or an `{ "error": … }`
    /// object on failure), as returned by the memory store.
    tags: serde_json::Value,
}

/// Input for `nexus_memory_vitality` — most-vital memories first.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryVitalityInput {
    /// Max results (default 50).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `nexus_memory_recall` — hybrid lexical + semantic recall.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryRecallInput {
    /// What to recall. Fused across full-text and (when available) vector search.
    query: String,
    /// Max results (default 20).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `nexus_memory_vector_sync` — embedding backfill.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryVectorSyncInput {
    /// Max memories to (re)index this call (default 1000).
    #[serde(default)]
    limit: Option<u32>,
}

/// Result of `nexus_memory_vector_sync`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemoryVectorSyncOutput {
    /// The store's reply (`{ "indexed": n }`, or `{ "error": … }` on failure).
    result: serde_json::Value,
}

/// Input for `nexus_memory_sync` — push/pull with a memory hub. All fields are
/// optional; omitted ones fall back to the `NEXUS_MEMORY_*` environment vars.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemorySyncInput {
    /// Hub base URL (else `NEXUS_MEMORY_HUB_URL`).
    #[serde(default)]
    hub_url: Option<String>,
    /// Shared bearer secret (else `NEXUS_MEMORY_SYNC_SECRET`).
    #[serde(default)]
    secret: Option<String>,
    /// This node's id (else `NEXUS_MEMORY_NODE_ID`).
    #[serde(default)]
    node_id: Option<String>,
}

/// Result of `nexus_memory_sync`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemorySyncOutput {
    /// The engine's reply (`{ "pushed": n, "pulled": n }`, or `{ "error": … }`).
    result: serde_json::Value,
}

/// Input for `nexus_memory_wiki_compile`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryWikiCompileInput {
    /// The page topic (also the slug + H1 title).
    topic: String,
    /// Optional search query for the source memories (defaults to `topic`).
    #[serde(default)]
    query: Option<String>,
    /// Max memories to synthesize from (default 30).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `nexus_memory_wiki_read`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryWikiReadInput {
    /// The page topic/slug to read.
    topic: String,
}

/// Wraps a wiki handler reply (page metadata, content, or page list — or
/// `{ "error": … }` on failure).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemoryWikiOutput {
    /// The memory plugin's wiki reply.
    result: serde_json::Value,
}

/// Input for `nexus_memory_capture`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryCaptureInput {
    /// The text/turn to capture verbatim.
    content: String,
    /// Also decompose it into atomic child facts via the LLM.
    #[serde(default)]
    decompose: Option<bool>,
    /// Originating client/provider label.
    #[serde(default)]
    client: Option<String>,
    /// Category for the captured memories.
    #[serde(default)]
    category: Option<String>,
}

/// Input for `nexus_memory_consolidate`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryConsolidateInput {
    /// Restrict to one category.
    #[serde(default)]
    category: Option<String>,
    /// Report what would be merged without changing anything.
    #[serde(default)]
    dry_run: Option<bool>,
}

/// Input for `nexus_memory_get`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemoryGetInput {
    /// The memory id (UUID string) to fetch.
    id: String,
}

/// Input for `nexus_memory_import` (C40 / #393).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemoryImportInput {
    /// Records to import — the array `nexus_memory_export` returns.
    records: Vec<serde_json::Value>,
}

/// Input for `nexus_memory_update` (C35). Only the provided fields change;
/// omitted fields keep their stored value.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemoryUpdateInput {
    /// The memory id (UUID string) to update.
    id: String,
    /// New content, if changing.
    #[serde(default)]
    content: Option<String>,
    /// New category, if changing.
    #[serde(default)]
    category: Option<String>,
    /// Replacement tag list, if changing.
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// New lifecycle status (`active` | `archived` | `superseded` |
    /// `deleted`), if changing. Prefer `nexus_memory_delete` over setting
    /// `deleted` here — same effect, clearer intent (C36).
    #[serde(default)]
    status: Option<String>,
    /// New cognitive class (`episodic` | `semantic` | `procedural` |
    /// `unclassified`), if reclassifying (C41 / #394).
    #[serde(default)]
    memory_type: Option<String>,
    /// New SPO subject, if changing.
    #[serde(default)]
    subject: Option<String>,
    /// New SPO predicate, if changing.
    #[serde(default)]
    predicate: Option<String>,
    /// New SPO object, if changing.
    #[serde(default)]
    object: Option<String>,
}

/// Input for `nexus_memory_delete` (C35) — "forget this memory".
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MemoryDeleteInput {
    /// The memory id (UUID string) to delete.
    id: String,
}

/// Wraps a memory operation's JSON reply (or `{ "error": … }` on failure).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemoryResultOutput {
    /// The memory plugin's reply.
    result: serde_json::Value,
}

/// Input for `nexus_sandbox_policy`. No parameters.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct SandboxPolicyInput {}

/// Input for `nexus_sandbox_download`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct SandboxDownloadInput {
    /// Source URL (must be https and on the sandbox download allowlist).
    url: String,
    /// Destination path (must be inside a sandbox writable root).
    dest: String,
    /// Working directory for resolving writable roots; defaults to the
    /// destination's parent.
    #[serde(default)]
    cwd: Option<String>,
}

/// Wraps a sandbox handler's JSON reply (or `{ "error": … }` on failure).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SandboxResultOutput {
    /// The security plugin's reply.
    result: serde_json::Value,
}

/// Input for `nexus_memory_facts` — recall SPO entity facts.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryFactsInput {
    /// Optional subject filter (e.g. `ada`).
    #[serde(default)]
    subject: Option<String>,
    /// Optional predicate filter (e.g. `writes`).
    #[serde(default)]
    predicate: Option<String>,
    /// Optional object filter (e.g. `rust`).
    #[serde(default)]
    object: Option<String>,
    /// Max results (default 50).
    #[serde(default)]
    limit: Option<u32>,
}

/// Input for `nexus_memory_entities` — list distinct entities with fact counts.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct MemoryEntitiesInput {
    /// Max entities to return (default 50).
    #[serde(default)]
    limit: Option<u32>,
}

/// A list of entities with fact counts.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemoryEntitiesOutput {
    /// Distinct entities as `{ key, count }` objects (or an `{ "error": … }`
    /// object on failure), as returned by the memory store.
    entities: serde_json::Value,
}

/// A list of memories (raw store objects).
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemoryListOutput {
    /// Matching memories as returned by the memory store.
    memories: serde_json::Value,
}

/// A single stored memory.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct MemoryItemOutput {
    /// The stored memory object (or an `{ "error": … }` object on failure).
    memory: serde_json::Value,
}

/// Input for `nexus_export_html` (C66) — render a forge note to standalone HTML.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExportHtmlInput {
    /// Forge-relative path to the markdown note to render.
    path: String,
    /// Document title. Defaults to the source file's stem.
    #[serde(default)]
    title: Option<String>,
    /// Forge-relative output path. When given, the HTML is written there
    /// instead of being returned inline.
    #[serde(default)]
    dest: Option<String>,
}

/// Reply from `nexus_export_html`: either `{ html }` or `{ written, dest }`
/// (or an `{ "error": … }` object on failure), as returned by
/// `com.nexus.formats::export_html`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ExportHtmlOutput {
    /// The formats plugin's reply.
    result: serde_json::Value,
}

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
        name = "nexus_terminal_get_screen",
        description = "Read a terminal session's current visible screen (server-side VT grid) as text, plus the cursor position."
    )]
    async fn terminal_get_screen(
        &self,
        Parameters(input): Parameters<TerminalSessionInput>,
    ) -> Json<serde_json::Value> {
        Json(
            self.terminal_tool("get_screen", serde_json::json!({ "id": input.session_id }))
                .await,
        )
    }

    #[tool(
        name = "nexus_terminal_get_scrollback",
        description = "Read a terminal session's scrollback (lines that scrolled off the top), oldest first."
    )]
    async fn terminal_get_scrollback(
        &self,
        Parameters(input): Parameters<TerminalScrollbackInput>,
    ) -> Json<serde_json::Value> {
        Json(
            self.terminal_tool(
                "get_scrollback",
                serde_json::json!({ "id": input.session_id, "lines": input.lines }),
            )
            .await,
        )
    }

    #[tool(
        name = "nexus_terminal_get_cwd",
        description = "Read a terminal session's working directory as reported by the child via OSC 7."
    )]
    async fn terminal_get_cwd(
        &self,
        Parameters(input): Parameters<TerminalSessionInput>,
    ) -> Json<serde_json::Value> {
        Json(
            self.terminal_tool("get_cwd", serde_json::json!({ "id": input.session_id }))
                .await,
        )
    }

    #[tool(
        name = "nexus_terminal_get_cursor",
        description = "Read a terminal session's cursor position (col, row; zero-based)."
    )]
    async fn terminal_get_cursor(
        &self,
        Parameters(input): Parameters<TerminalSessionInput>,
    ) -> Json<serde_json::Value> {
        Json(
            self.terminal_tool("get_cursor", serde_json::json!({ "id": input.session_id }))
                .await,
        )
    }

    #[tool(
        name = "nexus_terminal_get_last_exit",
        description = "Read a terminal session's last finished command exit code and captured output (OSC 133)."
    )]
    async fn terminal_get_last_exit(
        &self,
        Parameters(input): Parameters<TerminalSessionInput>,
    ) -> Json<serde_json::Value> {
        Json(
            self.terminal_tool(
                "get_last_exit",
                serde_json::json!({ "id": input.session_id }),
            )
            .await,
        )
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
        name = "nexus_memory_search",
        description = "Full-text search the persistent cross-model memory store"
    )]
    async fn memory_search(
        &self,
        Parameters(input): Parameters<MemorySearchInput>,
    ) -> Json<MemoryListOutput> {
        let args = serde_json::json!({ "query": input.query, "limit": input.limit });
        match self.memory_call::<serde_json::Value>("search", args).await {
            Ok(memories) => Json(MemoryListOutput { memories }),
            Err(e) => Json(MemoryListOutput {
                memories: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_add",
        description = "Store a new memory in the persistent cross-model memory store"
    )]
    async fn memory_add(
        &self,
        Parameters(input): Parameters<MemoryAddInput>,
    ) -> Json<MemoryItemOutput> {
        let args = serde_json::json!({
            "content": input.content,
            "category": input.category,
            "tags": input.tags.unwrap_or_default(),
            "memory_type": input.memory_type,
            "subject": input.subject,
            "predicate": input.predicate,
            "object": input.object,
        });
        match self.memory_call::<serde_json::Value>("add", args).await {
            Ok(memory) => Json(MemoryItemOutput { memory }),
            Err(e) => Json(MemoryItemOutput {
                memory: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_recent",
        description = "List the most recent memories, newest first"
    )]
    async fn memory_recent(
        &self,
        Parameters(input): Parameters<MemoryRecentInput>,
    ) -> Json<MemoryListOutput> {
        let args = serde_json::json!({ "limit": input.limit, "tag": input.tag });
        match self.memory_call::<serde_json::Value>("list", args).await {
            Ok(memories) => Json(MemoryListOutput { memories }),
            Err(e) => Json(MemoryListOutput {
                memories: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_facts",
        description = "Recall SPO entity facts from memory, optionally filtered by subject, predicate, and/or object"
    )]
    async fn memory_facts(
        &self,
        Parameters(input): Parameters<MemoryFactsInput>,
    ) -> Json<MemoryListOutput> {
        let args = serde_json::json!({
            "subject": input.subject,
            "predicate": input.predicate,
            "object": input.object,
            "limit": input.limit,
        });
        match self.memory_call::<serde_json::Value>("facts", args).await {
            Ok(memories) => Json(MemoryListOutput { memories }),
            Err(e) => Json(MemoryListOutput {
                memories: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_entities",
        description = "List the distinct entities mentioned by memory's SPO facts, each with its fact count (most-frequent first)"
    )]
    async fn memory_entities(
        &self,
        Parameters(input): Parameters<MemoryEntitiesInput>,
    ) -> Json<MemoryEntitiesOutput> {
        let args = serde_json::json!({ "limit": input.limit });
        match self
            .memory_call::<serde_json::Value>("entities", args)
            .await
        {
            Ok(entities) => Json(MemoryEntitiesOutput { entities }),
            Err(e) => Json(MemoryEntitiesOutput {
                entities: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_tags",
        description = "List the distinct tags across memories, each with the number of memories carrying it (most-frequent first)"
    )]
    async fn memory_tags(
        &self,
        Parameters(input): Parameters<MemoryTagsInput>,
    ) -> Json<MemoryTagsOutput> {
        let args = serde_json::json!({ "limit": input.limit });
        match self.memory_call::<serde_json::Value>("tags", args).await {
            Ok(tags) => Json(MemoryTagsOutput { tags }),
            Err(e) => Json(MemoryTagsOutput {
                tags: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_vitality",
        description = "List active memories ranked by vitality (frequency + recency of recall) — the ones most likely to matter right now"
    )]
    async fn memory_vitality(
        &self,
        Parameters(input): Parameters<MemoryVitalityInput>,
    ) -> Json<MemoryListOutput> {
        let args = serde_json::json!({ "limit": input.limit });
        match self
            .memory_call::<serde_json::Value>("vitality_report", args)
            .await
        {
            Ok(memories) => Json(MemoryListOutput { memories }),
            Err(e) => Json(MemoryListOutput {
                memories: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_recall",
        description = "Hybrid recall over memory: fuses full-text and semantic (vector) search via Reciprocal Rank Fusion. The best general way to find relevant memories"
    )]
    async fn memory_recall(
        &self,
        Parameters(input): Parameters<MemoryRecallInput>,
    ) -> Json<MemoryListOutput> {
        let args = serde_json::json!({ "query": input.query, "limit": input.limit });
        match self.memory_call::<serde_json::Value>("recall", args).await {
            Ok(memories) => Json(MemoryListOutput { memories }),
            Err(e) => Json(MemoryListOutput {
                memories: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_vector_sync",
        description = "Backfill embeddings for stored memories so semantic recall has data to search. Run once after importing or bulk-adding memories"
    )]
    async fn memory_vector_sync(
        &self,
        Parameters(input): Parameters<MemoryVectorSyncInput>,
    ) -> Json<MemoryVectorSyncOutput> {
        let args = serde_json::json!({ "limit": input.limit });
        match self
            .memory_call::<serde_json::Value>("vector_sync", args)
            .await
        {
            Ok(result) => Json(MemoryVectorSyncOutput { result }),
            Err(e) => Json(MemoryVectorSyncOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_sync",
        description = "Sync the memory store with a central memory hub (push local + pull remote, last-write-wins). Hub URL/secret/node default to NEXUS_MEMORY_* env vars"
    )]
    async fn memory_sync(
        &self,
        Parameters(input): Parameters<MemorySyncInput>,
    ) -> Json<MemorySyncOutput> {
        let args = serde_json::json!({
            "hub_url": input.hub_url,
            "secret": input.secret,
            "node_id": input.node_id,
        });
        match self.memory_call::<serde_json::Value>("sync", args).await {
            Ok(result) => Json(MemorySyncOutput { result }),
            Err(e) => Json(MemorySyncOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_wiki_compile",
        description = "Synthesize a Markdown wiki page about a topic from related memories (saved to wiki/<slug>.md in the forge) and return its metadata"
    )]
    async fn memory_wiki_compile(
        &self,
        Parameters(input): Parameters<MemoryWikiCompileInput>,
    ) -> Json<MemoryWikiOutput> {
        let args = serde_json::json!({
            "topic": input.topic,
            "query": input.query,
            "limit": input.limit,
        });
        match self
            .memory_call::<serde_json::Value>("wiki_compile", args)
            .await
        {
            Ok(result) => Json(MemoryWikiOutput { result }),
            Err(e) => Json(MemoryWikiOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_wiki_read",
        description = "Read a synthesized wiki page's Markdown by topic/slug"
    )]
    async fn memory_wiki_read(
        &self,
        Parameters(input): Parameters<MemoryWikiReadInput>,
    ) -> Json<MemoryWikiOutput> {
        let args = serde_json::json!({ "topic": input.topic });
        match self
            .memory_call::<serde_json::Value>("wiki_read", args)
            .await
        {
            Ok(result) => Json(MemoryWikiOutput { result }),
            Err(e) => Json(MemoryWikiOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_wiki_list",
        description = "List the synthesized wiki pages (slugs + paths)"
    )]
    async fn memory_wiki_list(&self) -> Json<MemoryWikiOutput> {
        match self
            .memory_call::<serde_json::Value>("wiki_list", serde_json::json!({}))
            .await
        {
            Ok(result) => Json(MemoryWikiOutput { result }),
            Err(e) => Json(MemoryWikiOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_sandbox_policy",
        description = "Show the active OS-sandbox configuration (from sandbox.toml): the process-confinement mode (read-only / workspace-write / danger-full-access), writable roots, network access, and the brokered-download allowlist. Read-only introspection"
    )]
    async fn sandbox_policy(
        &self,
        Parameters(_input): Parameters<SandboxPolicyInput>,
    ) -> Json<SandboxResultOutput> {
        match self
            .security_call::<serde_json::Value>("sandbox_policy", serde_json::json!({}))
            .await
        {
            Ok(result) => Json(SandboxResultOutput { result }),
            Err(e) => Json(SandboxResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_sandbox_download",
        description = "Perform a brokered, allowlisted download into a sandbox writable root on behalf of a network-confined process. Doubly gated: the net.http capability plus the sandbox.toml host allowlist + writable-root checks. Returns { bytes_written }"
    )]
    async fn sandbox_download(
        &self,
        Parameters(input): Parameters<SandboxDownloadInput>,
    ) -> Json<SandboxResultOutput> {
        let args = serde_json::json!({ "url": input.url, "dest": input.dest, "cwd": input.cwd });
        match self
            .security_call::<serde_json::Value>("download", args)
            .await
        {
            Ok(result) => Json(SandboxResultOutput { result }),
            Err(e) => Json(SandboxResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_capture",
        description = "Capture a conversation turn / note as a memory; optionally decompose it into atomic facts (LLM). The deliberate 'remember this' call"
    )]
    async fn memory_capture(
        &self,
        Parameters(input): Parameters<MemoryCaptureInput>,
    ) -> Json<MemoryResultOutput> {
        let args = serde_json::json!({
            "content": input.content,
            "decompose": input.decompose,
            "client": input.client,
            "category": input.category,
        });
        match self
            .memory_call::<serde_json::Value>("auto_capture", args)
            .await
        {
            Ok(result) => Json(MemoryResultOutput { result }),
            Err(e) => Json(MemoryResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_consolidate",
        description = "Deduplicate memories: supersede exact (normalized) duplicates, keeping the freshest. Use dry_run to preview"
    )]
    async fn memory_consolidate(
        &self,
        Parameters(input): Parameters<MemoryConsolidateInput>,
    ) -> Json<MemoryResultOutput> {
        let args = serde_json::json!({ "category": input.category, "dry_run": input.dry_run });
        match self
            .memory_call::<serde_json::Value>("consolidate", args)
            .await
        {
            Ok(result) => Json(MemoryResultOutput { result }),
            Err(e) => Json(MemoryResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_export",
        description = "Export every stored memory as full records (oldest first), suitable for backup or re-import into another store"
    )]
    async fn memory_export(&self) -> Json<MemoryListOutput> {
        match self
            .memory_call::<serde_json::Value>("export", serde_json::json!({}))
            .await
        {
            Ok(memories) => Json(MemoryListOutput { memories }),
            Err(e) => Json(MemoryListOutput {
                memories: serde_json::json!({ "error": e }),
            }),
        }
    }

    // C40 (#393) — export had no re-import counterpart on any surface, so a
    // backup could never be restored and memories couldn't be merged into a
    // non-empty forge.
    #[tool(
        name = "nexus_memory_import",
        description = "Restore or merge memory records previously produced by nexus_memory_export. Last-write-wins per id: an incoming record only overwrites a local one when it's strictly newer, so replaying an old backup can't clobber newer local edits"
    )]
    async fn memory_import(
        &self,
        Parameters(input): Parameters<MemoryImportInput>,
    ) -> Json<MemoryResultOutput> {
        let args = serde_json::json!({ "records": input.records });
        match self.memory_call::<serde_json::Value>("import", args).await {
            Ok(result) => Json(MemoryResultOutput { result }),
            Err(e) => Json(MemoryResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_get",
        description = "Fetch a single memory by id (records access for vitality ranking)"
    )]
    async fn memory_get(
        &self,
        Parameters(input): Parameters<MemoryGetInput>,
    ) -> Json<MemoryItemOutput> {
        let args = serde_json::json!({ "id": input.id });
        match self.memory_call::<serde_json::Value>("get", args).await {
            Ok(memory) => Json(MemoryItemOutput { memory }),
            Err(e) => Json(MemoryItemOutput {
                memory: serde_json::json!({ "error": e }),
            }),
        }
    }

    // C35 (#388) — the update/delete IPC handlers already existed
    // (crates/nexus-memory/src/core_plugin.rs) but no MCP tool reached them,
    // so an agent asked to "forget this" or "fix that memory" had no path.
    #[tool(
        name = "nexus_memory_update",
        description = "Patch a stored memory's fields (content, category, tags, status, memory_type, SPO fact fields). Only provided fields change"
    )]
    async fn memory_update(
        &self,
        Parameters(input): Parameters<MemoryUpdateInput>,
    ) -> Json<MemoryResultOutput> {
        let args = serde_json::json!({
            "id": input.id,
            "content": input.content,
            "category": input.category,
            "tags": input.tags,
            "status": input.status,
            "memory_type": input.memory_type,
            "subject": input.subject,
            "predicate": input.predicate,
            "object": input.object,
        });
        match self.memory_call::<serde_json::Value>("update", args).await {
            Ok(result) => Json(MemoryResultOutput { result }),
            Err(e) => Json(MemoryResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_memory_delete",
        description = "Permanently forget a memory by id"
    )]
    async fn memory_delete(
        &self,
        Parameters(input): Parameters<MemoryDeleteInput>,
    ) -> Json<MemoryResultOutput> {
        let args = serde_json::json!({ "id": input.id });
        match self.memory_call::<serde_json::Value>("delete", args).await {
            Ok(result) => Json(MemoryResultOutput { result }),
            Err(e) => Json(MemoryResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    // C66 (#419) — the styled single-note HTML exporter
    // (crates/nexus-formats/src/markdown/html.rs) had an IPC handler
    // (com.nexus.formats::export_html) but no MCP tool reached it.
    #[tool(
        name = "nexus_export_html",
        description = "Render a forge note to a standalone styled HTML document. Returns the HTML inline, or writes it to `dest` when given"
    )]
    async fn export_html(
        &self,
        Parameters(input): Parameters<ExportHtmlInput>,
    ) -> Json<ExportHtmlOutput> {
        let args = serde_json::json!({
            "source": input.path,
            "title": input.title,
            "dest": input.dest,
        });
        match self
            .formats_call::<serde_json::Value>("export_html", args)
            .await
        {
            Ok(result) => Json(ExportHtmlOutput { result }),
            Err(e) => Json(ExportHtmlOutput {
                result: serde_json::json!({ "error": e }),
            }),
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
            #[serde(default)]
            mtime: i64,
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
                serde_json::json!({
                    "query": &input.query,
                    "limit": limit,
                    "offset": input.offset,
                    "sort": input.sort,
                    "mtime_after": input.mtime_after,
                    "mtime_before": input.mtime_before,
                }),
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
                        mtime: h.mtime,
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

    // ── C48 (#401) — forge entity graph, read-only ────────────────────
    //
    // Thin wrappers over `com.nexus.storage::entity_get` /
    // `::entity_search` / `::entity_relations` — already classified
    // `unrestricted` read-only in cap_matrix.toml, same posture as
    // backlinks/outgoing_links above. This is the typed, confidence-
    // weighted entity graph the in-process agent already consumes
    // (`entity_recall`); external MCP clients previously had no way to
    // reach it, only the wikilink-only backlinks/outgoing_links pair.

    #[tool(
        name = "nexus_entity_get",
        description = "Fetch a forge entity by canonical id or alias: type, aliases, description, and outgoing relations"
    )]
    async fn entity_get(&self, Parameters(input): Parameters<EntityGetInput>) -> Json<EntityGetOutput> {
        #[derive(Deserialize)]
        struct Rel {
            target: String,
            #[serde(rename = "type")]
            kind: String,
            confidence: f32,
        }
        #[derive(Deserialize)]
        struct Rec {
            id: String,
            entity_type: String,
            aliases: Vec<String>,
            description: String,
            relations: Vec<Rel>,
            relpath: String,
        }
        #[derive(Deserialize)]
        struct GetReply {
            entity: Option<Rec>,
        }
        match self
            .storage_call::<GetReply>("entity_get", serde_json::json!({ "id": &input.id }))
            .await
        {
            Ok(reply) => Json(EntityGetOutput {
                entity: reply.entity.map(|r| EntityRecord {
                    id: r.id,
                    entity_type: r.entity_type,
                    aliases: r.aliases,
                    description: r.description,
                    relations: r
                        .relations
                        .into_iter()
                        .map(|rel| EntityRelationEntry {
                            target: rel.target,
                            kind: rel.kind,
                            confidence: rel.confidence,
                        })
                        .collect(),
                    relpath: r.relpath,
                }),
            }),
            Err(e) => {
                tracing::error!("entity_get failed: {e}");
                Json(EntityGetOutput { entity: None })
            }
        }
    }

    #[tool(
        name = "nexus_entity_search",
        description = "Search forge entities by substring against id / aliases / description, optionally filtered by entity_type"
    )]
    async fn entity_search(
        &self,
        Parameters(input): Parameters<EntitySearchInput>,
    ) -> Json<EntitySearchOutput> {
        #[derive(Deserialize)]
        struct Hit {
            id: String,
            entity_type: String,
            description: String,
            relpath: String,
            score: i32,
        }
        #[derive(Deserialize)]
        struct SearchReply {
            results: Vec<Hit>,
        }
        let args = serde_json::json!({
            "query": input.query,
            "entity_type": input.entity_type,
            "limit": input.limit,
        });
        match self.storage_call::<SearchReply>("entity_search", args).await {
            Ok(reply) => {
                let results: Vec<EntitySearchHit> = reply
                    .results
                    .into_iter()
                    .map(|h| EntitySearchHit {
                        id: h.id,
                        entity_type: h.entity_type,
                        description: h.description,
                        relpath: h.relpath,
                        score: h.score,
                    })
                    .collect();
                Json(EntitySearchOutput {
                    count: results.len(),
                    results,
                })
            }
            Err(e) => {
                tracing::error!("entity_search failed: {e}");
                Json(EntitySearchOutput {
                    count: 0,
                    results: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_entity_relations",
        description = "List an entity's relations (outgoing / incoming / both), alias-resolved to canonical ids"
    )]
    async fn entity_relations(
        &self,
        Parameters(input): Parameters<EntityRelationsInput>,
    ) -> Json<EntityRelationsOutput> {
        #[derive(Deserialize)]
        struct Row {
            from: String,
            to: String,
            #[serde(rename = "type")]
            kind: String,
            confidence: f32,
        }
        #[derive(Deserialize)]
        struct RelationsReply {
            relations: Vec<Row>,
        }
        let args = serde_json::json!({
            "id": &input.id,
            "direction": input.direction,
        });
        match self
            .storage_call::<RelationsReply>("entity_relations", args)
            .await
        {
            Ok(reply) => {
                let relations: Vec<EntityRelationsRow> = reply
                    .relations
                    .into_iter()
                    .map(|r| EntityRelationsRow {
                        from: r.from,
                        to: r.to,
                        kind: r.kind,
                        confidence: r.confidence,
                    })
                    .collect();
                Json(EntityRelationsOutput {
                    count: relations.len(),
                    relations,
                })
            }
            Err(e) => {
                tracing::error!("entity_relations failed: {e}");
                Json(EntityRelationsOutput {
                    count: 0,
                    relations: Vec::new(),
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

    // C78 (#431) — nexus_ask synthesizes a chat answer with only a
    // source *count*; agents/scripts that want the raw ranked hits
    // (file_path, score, excerpt) for their own use had no MCP tool
    // to call — this exposes the same com.nexus.ai::semantic_search
    // handler the shell's "Search by Meaning" already uses, with
    // `hybrid: true` fusing in lexical BM25 via RRF instead of
    // vector-only retrieval.
    #[tool(
        name = "nexus_semantic_search",
        description = "Embedding-driven retrieval over your notes: returns raw ranked chunk hits (file_path, score, excerpt/chunk_text) rather than a synthesized answer. Pass hybrid=true to fuse vector similarity with lexical BM25 (RRF) instead of vector-only. Requires an AI embedding provider to be configured."
    )]
    async fn nexus_semantic_search(
        &self,
        Parameters(input): Parameters<SemanticSearchInput>,
    ) -> Json<SemanticSearchOutput> {
        let args = serde_json::json!({
            "query": input.query,
            "limit": input.limit,
            "hybrid": input.hybrid,
        });
        match self
            .context
            .ipc_call(AI_PLUGIN, "semantic_search", args, AI_IPC_TIMEOUT)
            .await
        {
            Ok(result) => Json(SemanticSearchOutput { result }),
            Err(e) => Json(SemanticSearchOutput {
                result: serde_json::json!({ "error": e.to_string() }),
            }),
        }
    }

    #[tool(
        name = "nexus_list_skills",
        description = "List all skills (authored prompt templates) declared in the forge's .forge/skills directory"
    )]
    async fn list_skills(
        &self,
        Parameters(_input): Parameters<ListSkillsInput>,
    ) -> Json<ListSkillsOutput> {
        // Skills `list` returns the skill metadata directly — fields
        // mirror the `Skill::meta` shape in nexus-skills.
        #[derive(Deserialize)]
        struct Rec {
            id: String,
            name: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            version: String,
            #[serde(default)]
            tags: Vec<String>,
            #[serde(default)]
            applicable_contexts: Vec<String>,
        }
        match self
            .skills_call::<Vec<Rec>>("list", serde_json::json!({}))
            .await
        {
            Ok(records) => {
                let skills: Vec<SkillEntry> = records
                    .into_iter()
                    .map(|r| SkillEntry {
                        id: r.id,
                        name: r.name,
                        description: r.description,
                        version: r.version,
                        tags: r.tags,
                        applicable_contexts: r.applicable_contexts,
                    })
                    .collect();
                Json(ListSkillsOutput {
                    count: skills.len(),
                    skills,
                })
            }
            Err(e) => {
                tracing::error!("list_skills failed: {e}");
                Json(ListSkillsOutput {
                    count: 0,
                    skills: Vec::new(),
                })
            }
        }
    }

    #[tool(
        name = "nexus_render_skill",
        description = "Render a skill template to its expanded prompt body, given an optional `values` map of placeholder substitutions"
    )]
    async fn render_skill(
        &self,
        Parameters(input): Parameters<RenderSkillInput>,
    ) -> Json<RenderSkillOutput> {
        #[derive(Deserialize)]
        struct Rec {
            id: String,
            name: String,
            body: String,
        }
        let args = serde_json::json!({
            "id": &input.id,
            "values": input.values,
        });
        match self.skills_call::<Rec>("render", args).await {
            Ok(r) => Json(RenderSkillOutput {
                id: r.id,
                name: r.name,
                body: r.body,
            }),
            Err(e) => Json(RenderSkillOutput {
                id: input.id,
                name: String::new(),
                body: format!("Error: {e}"),
            }),
        }
    }

    // ── BL-115 code-intel tools ────────────────────────────────────────

    #[tool(
        name = "nexus_context",
        description = "Resolve a code symbol from the BL-114 index and return its source location, doc comment, enclosing impl/class/module, and sibling symbols (other methods on the same impl). Pass `name` plus an optional `path` to disambiguate symbols defined in multiple files."
    )]
    async fn nexus_context(
        &self,
        Parameters(input): Parameters<NexusContextInput>,
    ) -> Json<NexusContextOutput> {
        let rows = match self
            .query_symbol_rows(&input.name, input.path.as_deref(), 50)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("nexus_context: {e}");
                return Json(NexusContextOutput {
                    matches: vec![],
                    degraded: true,
                    degraded_reason: Some(format!("symbol query failed: {e}")),
                });
            }
        };
        let mut matches = Vec::with_capacity(rows.len());
        for row in &rows {
            matches.push(self.build_symbol_context(row).await);
        }
        Json(NexusContextOutput {
            matches,
            degraded: true,
            degraded_reason: Some(BL115_DEGRADED_REASON.to_string()),
        })
    }

    #[tool(
        name = "nexus_impact",
        description = "Assess the blast radius of changing a symbol. v1 uses a kind-based heuristic (functions are MEDIUM, traits/interfaces HIGH, modules/impls CRITICAL, methods LOW, …) and surfaces sibling symbols as a proxy for direct callers. Returns a `degraded` flag because BL-114's index does not yet carry call-edges; agents should temper recommendations accordingly. `depth` is accepted but treated as `1`."
    )]
    async fn nexus_impact(
        &self,
        Parameters(input): Parameters<NexusImpactInput>,
    ) -> Json<NexusImpactOutput> {
        let rows = match self
            .query_symbol_rows(&input.name, input.path.as_deref(), 50)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("nexus_impact: {e}");
                return Json(NexusImpactOutput {
                    matches: vec![],
                    degraded: true,
                    degraded_reason: Some(format!("symbol query failed: {e}")),
                });
            }
        };
        let mut matches = Vec::with_capacity(rows.len());
        for row in &rows {
            let ctx = self.build_symbol_context(row).await;
            let (risk, reason) = risk_for_kind(&row.kind);
            let direct = ctx.siblings.iter().map(clone_symbol_ref).collect();
            matches.push(ImpactReport {
                symbol: ctx,
                risk: risk.to_string(),
                risk_reason: reason.to_string(),
                direct_affected: direct,
            });
        }
        Json(NexusImpactOutput {
            matches,
            degraded: true,
            degraded_reason: Some(BL115_DEGRADED_REASON.to_string()),
        })
    }

    #[tool(
        name = "nexus_detect_changes",
        description = "List uncommitted forge files plus every BL-114 indexed symbol that lives in them. Powers a pre-commit blast-radius preview: an agent can run this before editing to know which code-symbols the user has already touched in their working tree."
    )]
    async fn nexus_detect_changes(
        &self,
        Parameters(_input): Parameters<NexusDetectChangesInput>,
    ) -> Json<NexusDetectChangesOutput> {
        #[derive(Deserialize)]
        struct StatusEntry {
            path: String,
        }
        let statuses = match self
            .git_call::<Vec<StatusEntry>>("file_statuses", serde_json::json!({}))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("nexus_detect_changes git: {e}");
                return Json(NexusDetectChangesOutput {
                    changed_files: vec![],
                    affected_symbols: vec![],
                    total_dirty: 0,
                    degraded: true,
                    degraded_reason: Some(format!("git statuses unavailable: {e}")),
                });
            }
        };
        let changed_files: Vec<String> = statuses.iter().map(|s| s.path.clone()).collect();
        let total_dirty = changed_files.len();

        let mut affected: Vec<SymbolRef> = Vec::new();
        for path in &changed_files {
            match self
                .storage_call::<QuerySymbolReply>(
                    "query_symbol",
                    serde_json::json!({ "path": path, "limit": 500 }),
                )
                .await
            {
                Ok(reply) => {
                    for row in reply.symbols {
                        affected.push(SymbolRef {
                            id: row.id,
                            name: row.name,
                            kind: row.kind,
                            line_start: row.line_start,
                        });
                    }
                }
                Err(e) => {
                    tracing::debug!("query_symbol for {path}: {e}");
                }
            }
        }

        Json(NexusDetectChangesOutput {
            changed_files,
            affected_symbols: affected,
            total_dirty,
            degraded: true,
            degraded_reason: Some(BL115_DEGRADED_REASON.to_string()),
        })
    }

    // C49 (#402) — GitEngine's fetch/pull/remotes existed with full
    // SSH-agent + keyring credential support but had no IPC handler at
    // all, so no frontend — shell, TUI, or MCP — could reach them; only
    // the CLI could, via a direct (non-IPC) GitEngine call. These three
    // tools are thin wrappers over the new com.nexus.git handlers.
    #[tool(
        name = "nexus_git_remotes",
        description = "List the forge repository's configured git remote names (e.g. [\"origin\"]). Read-only, no network access."
    )]
    async fn nexus_git_remotes(
        &self,
        Parameters(_input): Parameters<NexusGitRemotesInput>,
    ) -> Json<GitOkOutput> {
        match self
            .git_call::<serde_json::Value>("remotes", serde_json::json!({}))
            .await
        {
            Ok(result) => Json(GitOkOutput { result }),
            Err(e) => Json(GitOkOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_git_fetch",
        description = "Fetch all refs from a remote into the forge repository's remote-tracking branches. Does not modify the working tree or HEAD — use nexus_git_pull to also merge."
    )]
    async fn nexus_git_fetch(
        &self,
        Parameters(input): Parameters<NexusGitFetchInput>,
    ) -> Json<GitOkOutput> {
        match self
            .git_call::<serde_json::Value>("fetch", serde_json::json!({ "remote": input.remote }))
            .await
        {
            Ok(result) => Json(GitOkOutput { result }),
            Err(e) => Json(GitOkOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_git_pull",
        description = "Fetch from a remote and merge the named branch into HEAD. Returns { fast_forward, conflicts, commit_hash } — a non-empty `conflicts` list means the merge paused mid-flight and needs manual resolution (or com.nexus.git::abort_merge)."
    )]
    async fn nexus_git_pull(
        &self,
        Parameters(input): Parameters<NexusGitPullInput>,
    ) -> Json<GitOkOutput> {
        match self
            .git_call::<serde_json::Value>(
                "pull",
                serde_json::json!({ "remote": input.remote, "branch": input.branch }),
            )
            .await
        {
            Ok(result) => Json(GitOkOutput { result }),
            Err(e) => Json(GitOkOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    // C74 (#427) — com.nexus.comments exposed 7 IPC handlers but the
    // sole frontend consumer was the shell (commentsApi.ts); an AI
    // reviewer over MCP had to edit note files instead of using this
    // non-destructive annotation channel. These 7 tools are thin
    // mappings over the existing handlers, except create_thread,
    // which needs an anchor-resolution step (see below).
    #[tool(
        name = "nexus_comment_list",
        description = "List every comment thread on a note, each with its full reply history. Threads are anchored to a specific block; use nexus_comment_create_thread / nexus_comment_add_reply to write."
    )]
    async fn nexus_comment_list(
        &self,
        Parameters(input): Parameters<CommentListInput>,
    ) -> Json<CommentResultOutput> {
        match self
            .comments_call::<serde_json::Value>(
                "list",
                serde_json::json!({ "file_path": input.file_path }),
            )
            .await
        {
            Ok(result) => Json(CommentResultOutput { result }),
            Err(e) => Json(CommentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_comment_create_thread",
        description = "Start a new comment thread on a note, anchored to one of its top-level blocks (block_index, 0-based, default 0 = the first block). Comments are file-as-truth JSON sidecars — a non-destructive annotation channel, distinct from editing the note body directly. Internally resolves the anchor via com.nexus.editor's open/get_tree/stamp_block chain, the same machinery the shell's comment pane uses."
    )]
    async fn nexus_comment_create_thread(
        &self,
        Parameters(input): Parameters<CommentCreateThreadInput>,
    ) -> Json<CommentResultOutput> {
        let block_index = input.block_index.unwrap_or(0) as usize;

        if let Err(e) = self
            .editor_call::<serde_json::Value>(
                "open",
                serde_json::json!({ "relpath": input.file_path }),
            )
            .await
        {
            return Json(CommentResultOutput {
                result: serde_json::json!({ "error": format!("editor open: {e}") }),
            });
        }

        #[derive(Deserialize)]
        struct TreeReply {
            tree: TreeBody,
        }
        #[derive(Deserialize)]
        struct TreeBody {
            root_blocks: Vec<String>,
        }
        let tree = match self
            .editor_call::<TreeReply>(
                "get_tree",
                serde_json::json!({ "relpath": input.file_path }),
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                return Json(CommentResultOutput {
                    result: serde_json::json!({ "error": format!("editor get_tree: {e}") }),
                })
            }
        };
        let Some(block_id) = tree.tree.root_blocks.get(block_index) else {
            return Json(CommentResultOutput {
                result: serde_json::json!({
                    "error": format!(
                        "block_index {block_index} out of range — '{}' has {} top-level block(s)",
                        input.file_path,
                        tree.tree.root_blocks.len(),
                    ),
                }),
            });
        };

        #[derive(Deserialize)]
        struct StampReply {
            stable_id: String,
        }
        let stamp = match self
            .editor_call::<StampReply>(
                "stamp_block",
                serde_json::json!({ "relpath": input.file_path, "block_id": block_id }),
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                return Json(CommentResultOutput {
                    result: serde_json::json!({ "error": format!("editor stamp_block: {e}") }),
                })
            }
        };

        let args = serde_json::json!({
            "file_path": input.file_path,
            "block_id": stamp.stable_id,
            "body": input.body,
            "author": input.author,
        });
        match self
            .comments_call::<serde_json::Value>("create_thread", args)
            .await
        {
            Ok(result) => Json(CommentResultOutput { result }),
            Err(e) => Json(CommentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_comment_add_reply",
        description = "Append a reply to an existing comment thread."
    )]
    async fn nexus_comment_add_reply(
        &self,
        Parameters(input): Parameters<CommentAddReplyInput>,
    ) -> Json<CommentResultOutput> {
        let args = serde_json::json!({
            "file_path": input.file_path,
            "thread_id": input.thread_id,
            "body": input.body,
            "author": input.author,
        });
        match self
            .comments_call::<serde_json::Value>("add_reply", args)
            .await
        {
            Ok(result) => Json(CommentResultOutput { result }),
            Err(e) => Json(CommentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_comment_set_resolved",
        description = "Mark a comment thread resolved or unresolved."
    )]
    async fn nexus_comment_set_resolved(
        &self,
        Parameters(input): Parameters<CommentSetResolvedInput>,
    ) -> Json<CommentResultOutput> {
        let args = serde_json::json!({
            "file_path": input.file_path,
            "thread_id": input.thread_id,
            "resolved": input.resolved,
            "author": input.author,
        });
        match self
            .comments_call::<serde_json::Value>("set_resolved", args)
            .await
        {
            Ok(result) => Json(CommentResultOutput { result }),
            Err(e) => Json(CommentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_comment_edit_comment",
        description = "Edit an existing comment's body in place."
    )]
    async fn nexus_comment_edit_comment(
        &self,
        Parameters(input): Parameters<CommentEditCommentInput>,
    ) -> Json<CommentResultOutput> {
        let args = serde_json::json!({
            "file_path": input.file_path,
            "thread_id": input.thread_id,
            "comment_id": input.comment_id,
            "body": input.body,
        });
        match self
            .comments_call::<serde_json::Value>("edit_comment", args)
            .await
        {
            Ok(result) => Json(CommentResultOutput { result }),
            Err(e) => Json(CommentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_comment_delete_comment",
        description = "Delete a single comment from a thread. Deleting a thread's only comment leaves an empty thread — use nexus_comment_delete_thread to remove the whole thread instead."
    )]
    async fn nexus_comment_delete_comment(
        &self,
        Parameters(input): Parameters<CommentDeleteCommentInput>,
    ) -> Json<CommentResultOutput> {
        let args = serde_json::json!({
            "file_path": input.file_path,
            "thread_id": input.thread_id,
            "comment_id": input.comment_id,
        });
        match self
            .comments_call::<serde_json::Value>("delete_comment", args)
            .await
        {
            Ok(result) => Json(CommentResultOutput { result }),
            Err(e) => Json(CommentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_comment_delete_thread",
        description = "Delete an entire comment thread, including all its replies."
    )]
    async fn nexus_comment_delete_thread(
        &self,
        Parameters(input): Parameters<CommentDeleteThreadInput>,
    ) -> Json<CommentResultOutput> {
        let args = serde_json::json!({
            "file_path": input.file_path,
            "thread_id": input.thread_id,
        });
        match self
            .comments_call::<serde_json::Value>("delete_thread", args)
            .await
        {
            Ok(result) => Json(CommentResultOutput { result }),
            Err(e) => Json(CommentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    // C75 (#428) — the MCP server's static tools previously covered
    // notes/graph/tasks/RAG/skills/git/security/memory/comments but
    // could not drive either of Nexus's two execution surfaces: an
    // agent session or a user-authored workflow. `nexus_agent_run`
    // always sets `auto_approve: true` — a synchronous MCP tool call
    // has no bidirectional channel for the CLI's interactive
    // round_proposed/round_decide approval loop (BL-132), so it
    // mirrors the CLI's non-interactive default instead.
    #[tool(
        name = "nexus_agent_run",
        description = "Delegate a goal to a Nexus agent session and run it end-to-end (auto-approving every tool call — there is no interactive approval channel over MCP). Returns the full transcript: rounds, tool calls, and outcome. Archetype selects the system prompt / tool profile (writer / coder / researcher / general, default general)."
    )]
    async fn nexus_agent_run(
        &self,
        Parameters(input): Parameters<AgentRunInput>,
    ) -> Json<AgentResultOutput> {
        let args = serde_json::json!({
            "goal": input.goal,
            "archetype": input.archetype,
            "auto_approve": true,
        });
        match self
            .agent_call::<serde_json::Value>("session_run", args, RUN_IPC_TIMEOUT)
            .await
        {
            Ok(result) => Json(AgentResultOutput { result }),
            Err(e) => Json(AgentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_agent_sessions",
        description = "List stored agent sessions — id, outcome, goal, and fork lineage (parent_id / branch_point) for sessions created via resume/branch/rewind. Read-only."
    )]
    async fn nexus_agent_sessions(
        &self,
        Parameters(_input): Parameters<AgentSessionsInput>,
    ) -> Json<AgentResultOutput> {
        match self
            .agent_call::<serde_json::Value>("session_list", serde_json::json!({}), IPC_TIMEOUT)
            .await
        {
            Ok(result) => Json(AgentResultOutput { result }),
            Err(e) => Json(AgentResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_workflow_list",
        description = "List every loaded `.workflow.toml` — name, trigger config, and step count. Read-only."
    )]
    async fn nexus_workflow_list(
        &self,
        Parameters(_input): Parameters<WorkflowListInput>,
    ) -> Json<WorkflowResultOutput> {
        match self
            .workflow_call::<serde_json::Value>("list", serde_json::json!({}), IPC_TIMEOUT)
            .await
        {
            Ok(result) => Json(WorkflowResultOutput { result }),
            Err(e) => Json(WorkflowResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_workflow_run",
        description = "Fire a user-authored workflow on demand and run every step to completion. Each step is still gated by its own target handler's capabilities (issue #77 — the workflow boundary itself imposes no additional cap ceiling), so this can have side effects as broad as the workflow's steps."
    )]
    async fn nexus_workflow_run(
        &self,
        Parameters(input): Parameters<WorkflowRunInput>,
    ) -> Json<WorkflowResultOutput> {
        let args = serde_json::json!({ "name": input.name });
        match self
            .workflow_call::<serde_json::Value>("run", args, RUN_IPC_TIMEOUT)
            .await
        {
            Ok(result) => Json(WorkflowResultOutput { result }),
            Err(e) => Json(WorkflowResultOutput {
                result: serde_json::json!({ "error": e }),
            }),
        }
    }

    #[tool(
        name = "nexus_kernel_stats",
        description = "Snapshot the kernel's BL-093 metrics: per-(plugin, command) IPC call counters + duration histograms (p50/p95/p99), event-bus publish counters, capability-check counters by outcome, plugin-lifecycle-hook histograms, current event-bus queue depth, and `metrics_dropped_total` (sentinel for the per-metric key cap). Read-only. Useful for monitoring kernel hot paths or diagnosing latency / capability-deny regressions from an agent."
    )]
    async fn nexus_kernel_stats(
        &self,
        Parameters(_input): Parameters<NexusKernelStatsInput>,
    ) -> Json<NexusKernelStatsOutput> {
        match self
            .security_call::<serde_json::Value>("metrics_snapshot", serde_json::json!({}))
            .await
        {
            Ok(v) if v.is_null() => Json(NexusKernelStatsOutput { snapshot: None }),
            Ok(v) => Json(NexusKernelStatsOutput { snapshot: Some(v) }),
            Err(e) => {
                tracing::error!("nexus_kernel_stats: {e}");
                Json(NexusKernelStatsOutput { snapshot: None })
            }
        }
    }

    /// Shared symbol query: `path` is optional, `limit` is the hard
    /// cap. Returns the raw rows decoded straight from
    /// `com.nexus.storage::query_symbol`.
    async fn query_symbol_rows(
        &self,
        name: &str,
        path: Option<&str>,
        limit: u32,
    ) -> Result<Vec<QuerySymbolRow>, String> {
        let mut args = serde_json::json!({ "name": name, "limit": limit });
        if let Some(p) = path {
            if let Some(obj) = args.as_object_mut() {
                obj.insert("path".to_string(), serde_json::json!(p));
            }
        }
        let reply: QuerySymbolReply = self.storage_call("query_symbol", args).await?;
        Ok(reply.symbols)
    }

    /// Resolve parent + path-mate siblings for a symbol row and
    /// return the [`SymbolContext`] view.
    async fn build_symbol_context(&self, row: &QuerySymbolRow) -> SymbolContext {
        let mates: Vec<QuerySymbolRow> = self
            .storage_call::<QuerySymbolReply>(
                "query_symbol",
                serde_json::json!({ "path": row.path, "limit": 500 }),
            )
            .await
            .map(|r| r.symbols)
            .unwrap_or_default();

        let parent: Option<SymbolRef> = row.parent_id.and_then(|pid| {
            mates.iter().find(|m| m.id == pid).map(|m| SymbolRef {
                id: m.id,
                name: m.name.clone(),
                kind: m.kind.clone(),
                line_start: m.line_start,
            })
        });

        // Siblings = symbols whose `parent_id` matches this row's
        // `parent_id`, excluding the row itself. For a top-level
        // symbol (no parent) we leave siblings empty rather than
        // surfacing every other top-level decl in the file as a
        // sibling — that would be noisy and not what GitNexus's
        // tool does.
        let siblings: Vec<SymbolRef> = if let Some(pid) = row.parent_id {
            mates
                .iter()
                .filter(|m| m.id != row.id && m.parent_id == Some(pid))
                .map(|m| SymbolRef {
                    id: m.id,
                    name: m.name.clone(),
                    kind: m.kind.clone(),
                    line_start: m.line_start,
                })
                .collect()
        } else {
            Vec::new()
        };

        SymbolContext {
            id: row.id,
            path: row.path.clone(),
            name: row.name.clone(),
            kind: row.kind.clone(),
            language: row.language.clone(),
            line_start: row.line_start,
            line_end: row.line_end,
            doc_comment: row.doc_comment.clone(),
            parent,
            siblings,
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
            // RFC 0003 — clients may subscribe to terminal resources and receive
            // notifications/resources/updated as the VT grid changes.
            .enable_resources_subscribe()
            // C32 (#385) — forge skills are also exposed as MCP prompts
            // (see `list_prompts`/`get_prompt` below) so native prompt
            // pickers (Claude Desktop, Cursor) surface them directly
            // instead of requiring the nexus_list_skills /
            // nexus_render_skill tool workaround.
            .enable_prompts()
            .build();
        info.with_instructions(
            "Nexus MCP server: manage a personal knowledge base of markdown notes. \
             Use nexus_* tools to create, read, update, delete, search, and query notes; \
             list and render authored skill templates from .forge/skills via \
             nexus_list_skills / nexus_render_skill, or as native prompts (skills are \
             also exposed 1:1 through prompts/list and prompts/get). Query the typed \
             forge entity graph (distinct from the wikilink-only backlinks / \
             outgoing_links pair) via nexus_entity_get / nexus_entity_search / \
             nexus_entity_relations. Forge notes are also enumerated as MCP resources \
             under mcp://nexus/notes/. Observe live terminal sessions with \
             nexus_terminal_get_screen / _scrollback / _cwd / _cursor / _last_exit \
             (OSC 133), also exposed as resources under mcp://nexus/terminal/<id>/.",
        )
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        // DG-40 / PRD-14 §12.2 — every tool call is audited with the
        // tool name and wall-clock duration. We capture the name before
        // moving the request into the ToolCallContext so it stays
        // available after the call finishes.
        let tool_name = request.name.to_string();
        let started = std::time::Instant::now();
        // DG-39 / PRD-14 §10 — dynamic registry beats the static
        // router. Static `nexus_*` tools can't collide because their
        // names are reserved by `dynamic_tools::validate_name`.
        let dynamic = crate::dynamic_tools::global().lookup(&tool_name);
        let kernel_ctx = Arc::clone(&self.context);
        let tcc_opt = dynamic.is_none().then(|| {
            rmcp::handler::server::tool::ToolCallContext::new(self, request.clone(), context)
        });
        let static_fut = tcc_opt.map(|tcc| self.tool_router.call(tcc));
        async move {
            let outcome = if let Some(tool) = dynamic {
                // Plugin-published tool — route through ipc_call.
                let args = request.arguments.map_or_else(
                    || serde_json::Value::Object(serde_json::Map::new()),
                    serde_json::Value::Object,
                );
                match kernel_ctx
                    .ipc_call(&tool.plugin_id, &tool.command, args, IPC_TIMEOUT)
                    .await
                {
                    Ok(value) => Ok(CallToolResult::structured(value)),
                    Err(e) => Err(rmcp::ErrorData::internal_error(
                        format!("dynamic tool '{}' failed: {e}", tool.name),
                        None,
                    )),
                }
            } else if let Some(fut) = static_fut {
                fut.await
            } else {
                // Defensive: unreachable in practice — either
                // `dynamic` is `Some` or `tcc_opt` was built above.
                Err(rmcp::ErrorData::method_not_found::<
                    rmcp::model::CallToolRequestMethod,
                >())
            };
            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            match &outcome {
                Ok(_) => {
                    nexus_kernel::audit::log_mcp_tool_call(&tool_name, duration_ms, "success", None)
                }
                Err(e) => nexus_kernel::audit::log_mcp_tool_call(
                    &tool_name,
                    duration_ms,
                    "error",
                    Some(&e.to_string()),
                ),
            }
            outcome
        }
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        // DG-39 / PRD-14 §10 — surface dynamic tools alongside the
        // static `nexus_*` router output. Dynamic entries are
        // appended; their order is alphabetical by name
        // (BTreeMap iteration) so external clients see a stable
        // ordering.
        let mut items = self.tool_router.list_all();
        for t in crate::dynamic_tools::global().list() {
            items.push(dynamic_tool_to_rmcp(&t));
        }
        std::future::ready(Ok(ListToolsResult {
            tools: items,
            ..Default::default()
        }))
    }

    // ── C32 (#385) — forge skills as MCP prompts ─────────────────────────
    //
    // `com.nexus.skills::list`/`::render` already back the
    // `nexus_list_skills`/`nexus_render_skill` tools above; this just
    // exposes the same data through the dedicated MCP prompts/list and
    // prompts/get methods so native prompt pickers see it without the
    // tool-call workaround. `Prompt::name` is the skill's stable `id`
    // (used as the lookup key by `get_prompt`, matching `render_skill`'s
    // `id` field); the human-readable `name` becomes `Prompt::title`.

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, rmcp::ErrorData> {
        // Mirrors `SkillParameter` (nexus-skills/src/lib.rs) — only the
        // fields that map onto `PromptArgument` are captured.
        #[derive(Deserialize)]
        struct SkillParameterRec {
            name: String,
            #[serde(default)]
            description: Option<String>,
        }
        #[derive(Deserialize)]
        struct Rec {
            id: String,
            name: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            parameters: Vec<SkillParameterRec>,
        }
        let records: Vec<Rec> = self
            .skills_call("list", serde_json::json!({}))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(format!("list_prompts: {e}"), None))?;
        let prompts = records
            .into_iter()
            .map(|r| {
                let arguments = (!r.parameters.is_empty()).then(|| {
                    r.parameters
                        .into_iter()
                        .map(|p| {
                            let mut arg = PromptArgument::new(p.name);
                            if let Some(desc) = p.description {
                                arg = arg.with_description(desc);
                            }
                            // Skills don't declare a `required` flag on
                            // parameters (only a `default`) — `render`
                            // treats every parameter as optional
                            // (falls back to its default), so every
                            // prompt argument is optional too.
                            arg
                        })
                        .collect()
                });
                Prompt::new(r.id, Some(r.description), arguments).with_title(r.name)
            })
            .collect();
        Ok(ListPromptsResult {
            prompts,
            ..Default::default()
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, rmcp::ErrorData> {
        #[derive(Deserialize)]
        struct Rec {
            name: String,
            body: String,
        }
        let args = serde_json::json!({
            "id": &request.name,
            "values": request.arguments.unwrap_or_default(),
        });
        let rec: Rec = self.skills_call("render", args).await.map_err(|e| {
            rmcp::ErrorData::invalid_params(
                format!("unknown or unrenderable prompt '{}': {e}", request.name),
                None,
            )
        })?;
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            rec.body,
        )])
        .with_description(rec.name))
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
        let mut resources: Vec<Resource> = records
            .into_iter()
            .map(|r| build_note_resource(&r.path, r.size_bytes))
            .collect();

        // RFC 0003 Track A — expose each live terminal session's VT-grid state as
        // resources. Best-effort: if the terminal plugin is absent or has no
        // sessions, just contribute none rather than failing the whole listing.
        #[derive(Deserialize)]
        struct TermSession {
            id: String,
        }
        let sessions: Vec<TermSession> = self
            .terminal_call("list_sessions", serde_json::json!({}))
            .await
            .unwrap_or_default();
        for s in sessions {
            for (kind, desc) in TERMINAL_RESOURCE_KINDS {
                resources.push(build_terminal_resource(&s.id, kind, desc));
            }
        }

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
        // DG-40 / PRD-14 §12.2 — audit every resource read with the
        // URI and wall-clock duration.
        let started = std::time::Instant::now();
        let uri = request.uri.clone();
        let outcome: Result<ReadResourceResult, rmcp::ErrorData> = async {
            // RFC 0003 Track A — terminal VT-grid resources.
            if let Some((id, kind)) = parse_terminal_uri(&uri) {
                let text = self.read_terminal_resource(id, kind).await.map_err(|e| {
                    rmcp::ErrorData::resource_not_found(
                        format!("resource not found: {uri} ({e})"),
                        None,
                    )
                })?;
                let contents = ResourceContents::text(text, &uri).with_mime_type("text/plain");
                return Ok(ReadResourceResult::new(vec![contents]));
            }
            let Some(path) = parse_note_uri(&uri) else {
                return Err(rmcp::ErrorData::resource_not_found(
                    format!("unknown resource uri: {uri}"),
                    None,
                ));
            };
            let resp: ReadFileResp = self
                .storage_call("read_file", serde_json::json!({ "path": path }))
                .await
                .map_err(|e| {
                    rmcp::ErrorData::resource_not_found(
                        format!("resource not found: {uri} ({e})"),
                        None,
                    )
                })?;
            let text = String::from_utf8_lossy(&resp.bytes).into_owned();
            let contents = ResourceContents::text(text, &uri).with_mime_type("text/markdown");
            Ok(ReadResourceResult::new(vec![contents]))
        }
        .await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match &outcome {
            Ok(_) => nexus_kernel::audit::log_mcp_resource_read(&uri, duration_ms, "success", None),
            Err(e) => nexus_kernel::audit::log_mcp_resource_read(
                &uri,
                duration_ms,
                "error",
                Some(&e.to_string()),
            ),
        }
        outcome
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_terminal_event_routes_each_kind() {
        // command_finished pushes all three resources.
        assert!(matches!(
            classify_terminal_event("command_finished"),
            NotifyAction::PushResources(ks) if ks == ["screen", "exit", "command"]
        ));
        // output_received is debounced screen-only.
        assert!(matches!(
            classify_terminal_event("output_received"),
            NotifyAction::ScreenDebounced
        ));
        // L4 regression: both session ends release the debounce slot — eviction
        // must not leak an entry like it did before this arm existed.
        assert!(matches!(
            classify_terminal_event("session_closed"),
            NotifyAction::Release
        ));
        assert!(matches!(
            classify_terminal_event("session_evicted"),
            NotifyAction::Release
        ));
        // Anything else is ignored.
        assert!(matches!(
            classify_terminal_event("session_created"),
            NotifyAction::Ignore
        ));
    }

    #[test]
    fn recv_error_lagged_is_recoverable_only_closed_is_terminal() {
        // H1 regression: a lag must NOT end the notifier loop (recoverable);
        // only a closed bus is terminal. Collapsing both killed notifications
        // permanently after one lag.
        assert!(!recv_error_is_terminal(&nexus_kernel::RecvError::Lagged(7)));
        assert!(recv_error_is_terminal(&nexus_kernel::RecvError::Closed));
    }

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
    fn parse_terminal_uri_extracts_id_and_kind() {
        assert_eq!(
            parse_terminal_uri("mcp://nexus/terminal/abc-123/screen"),
            Some(("abc-123", "screen"))
        );
        assert_eq!(
            parse_terminal_uri("mcp://nexus/terminal/s1/last"),
            Some(("s1", "last"))
        );
        // Non-terminal URIs and missing components are rejected.
        assert_eq!(parse_terminal_uri("mcp://nexus/notes/foo.md"), None);
        assert_eq!(parse_terminal_uri("mcp://nexus/terminal/abc"), None);
        assert_eq!(parse_terminal_uri("mcp://nexus/terminal//screen"), None);
    }

    #[test]
    fn build_terminal_resource_sets_uri_mime_and_name() {
        let r = build_terminal_resource("sess-1", "screen", "Current visible screen");
        assert_eq!(r.raw.uri, "mcp://nexus/terminal/sess-1/screen");
        assert_eq!(r.raw.mime_type.as_deref(), Some("text/plain"));
        assert!(r.raw.name.contains("sess-1"));
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

    #[test]
    fn render_skill_input_defaults_values_to_empty_map() {
        let input: RenderSkillInput = serde_json::from_value(serde_json::json!({
            "id": "skill-a"
        }))
        .unwrap();
        assert_eq!(input.id, "skill-a");
        assert!(input.values.is_empty());
    }

    #[test]
    fn render_skill_input_round_trips_values_map() {
        let input: RenderSkillInput = serde_json::from_value(serde_json::json!({
            "id": "skill-b",
            "values": { "topic": "rust", "tone": "concise" }
        }))
        .unwrap();
        assert_eq!(input.id, "skill-b");
        assert_eq!(input.values.len(), 2);
        assert_eq!(input.values["topic"], serde_json::json!("rust"));
    }

    #[test]
    fn comment_create_thread_input_defaults_block_index_and_author_to_none() {
        let input: CommentCreateThreadInput = serde_json::from_value(serde_json::json!({
            "file_path": "notes/a.md",
            "body": "first comment"
        }))
        .unwrap();
        assert_eq!(input.file_path, "notes/a.md");
        assert_eq!(input.body, "first comment");
        assert_eq!(input.block_index, None);
        assert_eq!(input.author, None);
    }

    #[test]
    fn comment_create_thread_input_round_trips_block_index_and_author() {
        let input: CommentCreateThreadInput = serde_json::from_value(serde_json::json!({
            "file_path": "notes/a.md",
            "body": "first comment",
            "block_index": 2,
            "author": "reviewer"
        }))
        .unwrap();
        assert_eq!(input.block_index, Some(2));
        assert_eq!(input.author.as_deref(), Some("reviewer"));
    }

    #[test]
    fn comment_add_reply_input_defaults_author_to_none() {
        let input: CommentAddReplyInput = serde_json::from_value(serde_json::json!({
            "file_path": "notes/a.md",
            "thread_id": "11111111-1111-1111-1111-111111111111",
            "body": "a reply"
        }))
        .unwrap();
        assert_eq!(input.author, None);
    }

    #[test]
    fn agent_run_input_defaults_archetype_to_none() {
        let input: AgentRunInput = serde_json::from_value(serde_json::json!({
            "goal": "summarize yesterday's notes"
        }))
        .unwrap();
        assert_eq!(input.goal, "summarize yesterday's notes");
        assert_eq!(input.archetype, None);
    }

    #[test]
    fn agent_run_input_round_trips_archetype() {
        let input: AgentRunInput = serde_json::from_value(serde_json::json!({
            "goal": "draft a proposal",
            "archetype": "writer"
        }))
        .unwrap();
        assert_eq!(input.archetype.as_deref(), Some("writer"));
    }

    #[test]
    fn workflow_run_input_requires_name() {
        let input: WorkflowRunInput = serde_json::from_value(serde_json::json!({
            "name": "daily-journal"
        }))
        .unwrap();
        assert_eq!(input.name, "daily-journal");
    }

    #[test]
    fn list_skills_output_serializes_count_and_skills() {
        let out = ListSkillsOutput {
            count: 1,
            skills: vec![SkillEntry {
                id: "s1".into(),
                name: "Skill One".into(),
                description: "first".into(),
                version: "1.0.0".into(),
                tags: vec!["alpha".into()],
                applicable_contexts: vec!["ai-chat".into()],
            }],
        };
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["count"], 1);
        assert_eq!(v["skills"][0]["id"], "s1");
        assert_eq!(v["skills"][0]["applicable_contexts"][0], "ai-chat");
    }

    #[test]
    fn render_skill_output_serializes_id_name_body() {
        let out = RenderSkillOutput {
            id: "s1".into(),
            name: "Skill One".into(),
            body: "rendered body".into(),
        };
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["id"], "s1");
        assert_eq!(v["name"], "Skill One");
        assert_eq!(v["body"], "rendered body");
    }

    // ── BL-115 code-intel tool tests ──────────────────────────────────────

    #[test]
    fn risk_for_kind_method_is_low() {
        let (risk, _) = risk_for_kind("method");
        assert_eq!(risk, "LOW");
    }

    #[test]
    fn risk_for_kind_function_is_medium() {
        let (risk, _) = risk_for_kind("function");
        assert_eq!(risk, "MEDIUM");
    }

    #[test]
    fn risk_for_kind_trait_is_high() {
        let (risk, _) = risk_for_kind("trait");
        assert_eq!(risk, "HIGH");
        let (risk2, _) = risk_for_kind("interface");
        assert_eq!(risk2, "HIGH");
    }

    #[test]
    fn risk_for_kind_module_and_impl_are_critical() {
        let (m_risk, _) = risk_for_kind("module");
        let (i_risk, _) = risk_for_kind("impl");
        assert_eq!(m_risk, "CRITICAL");
        assert_eq!(i_risk, "CRITICAL");
    }

    #[test]
    fn risk_for_kind_unknown_falls_back_to_medium() {
        let (risk, reason) = risk_for_kind("anything-novel");
        assert_eq!(risk, "MEDIUM");
        assert!(reason.contains("unrecognised"));
    }

    #[test]
    fn query_symbol_row_decodes_minimal_payload() {
        let row: QuerySymbolRow = serde_json::from_value(serde_json::json!({
            "id": 7,
            "path": "src/lib.rs",
            "language": "rust",
            "kind": "function",
            "name": "hello",
            "line_start": 12,
            "line_end": 15,
        }))
        .unwrap();
        assert_eq!(row.id, 7);
        assert_eq!(row.parent_id, None);
        assert!(row.doc_comment.is_none());
    }

    #[test]
    fn query_symbol_reply_decodes_envelope() {
        let reply: QuerySymbolReply = serde_json::from_value(serde_json::json!({
            "symbols": [
                {
                    "id": 1, "path": "a.rs", "language": "rust",
                    "kind": "function", "name": "a", "line_start": 1, "line_end": 2,
                },
                {
                    "id": 2, "path": "a.rs", "language": "rust",
                    "kind": "function", "name": "b", "line_start": 4, "line_end": 5,
                    "parent_id": 1, "doc_comment": "doc"
                },
            ]
        }))
        .unwrap();
        assert_eq!(reply.symbols.len(), 2);
        assert_eq!(reply.symbols[1].parent_id, Some(1));
        assert_eq!(reply.symbols[1].doc_comment.as_deref(), Some("doc"));
    }

    #[test]
    fn nexus_context_output_serializes_degraded_flag() {
        let out = NexusContextOutput {
            matches: vec![],
            degraded: true,
            degraded_reason: Some("missing call edges".into()),
        };
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["degraded"], true);
        assert!(v["degraded_reason"].as_str().unwrap().contains("call"));
    }

    #[test]
    fn impact_report_serializes_risk_and_siblings() {
        let report = ImpactReport {
            symbol: SymbolContext {
                id: 10,
                path: "src/lib.rs".into(),
                name: "Counter".into(),
                kind: "struct".into(),
                language: "rust".into(),
                line_start: 1,
                line_end: 3,
                doc_comment: None,
                parent: None,
                siblings: vec![],
            },
            risk: "MEDIUM".into(),
            risk_reason: "data-shape symbol".into(),
            direct_affected: vec![SymbolRef {
                id: 11,
                name: "new".into(),
                kind: "method".into(),
                line_start: 5,
            }],
        };
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["symbol"]["name"], "Counter");
        assert_eq!(v["risk"], "MEDIUM");
        assert_eq!(v["direct_affected"][0]["name"], "new");
    }

    #[test]
    fn detect_changes_output_carries_total_dirty() {
        let out = NexusDetectChangesOutput {
            changed_files: vec!["a.rs".into(), "b.rs".into()],
            affected_symbols: vec![SymbolRef {
                id: 1,
                name: "foo".into(),
                kind: "function".into(),
                line_start: 1,
            }],
            total_dirty: 2,
            degraded: true,
            degraded_reason: None,
        };
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["total_dirty"], 2);
        assert_eq!(v["changed_files"].as_array().unwrap().len(), 2);
        assert_eq!(v["affected_symbols"][0]["name"], "foo");
    }

    #[test]
    fn bl115_degraded_reason_is_informative() {
        assert!(BL115_DEGRADED_REASON.contains("BL-114"));
        assert!(BL115_DEGRADED_REASON.contains("call-edge"));
    }
}
