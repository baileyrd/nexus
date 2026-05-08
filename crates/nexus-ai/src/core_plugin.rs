//! Core plugin wrapper for the AI engine.
//!
//! Registers as `com.nexus.ai`. Detects the AI + embedding providers from
//! environment on `on_init` and exposes async IPC commands for the rest of
//! the runtime:
//!
//! | Command                 | Handler id | Description                    |
//! |-------------------------|------------|--------------------------------|
//! | `ask`                   | 1          | RAG: embed → search → chat     |
//! | `index_file`            | 2          | Chunk, embed, upsert vectors   |
//! | `vectorstore_count`     | 3          | Count indexed chunks           |
//! | `status`                | 4          | Summary of provider + indexed  |
//! | `config`                | 5          | Detected provider snapshot     |
//!
//! All five are async handlers — they issue `com.nexus.storage` IPC calls
//! for vector ops and HTTP requests to the provider APIs.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::Serialize;

use crate::activity_log::ActivityRecorder;
use nexus_types::activity::{ActivityEntry, ActivityOutcome, ActivitySurface};
use crate::anthropic::AnthropicProvider;
use crate::config::{detect_embedding_provider, detect_provider, AiConfig};
use crate::embedding::EmbeddingProvider;
use crate::indexing_daemon::{self, DaemonMsg, EmbedderFactory, IndexingDaemon, SharedStatus};
use tokio::sync::mpsc::UnboundedSender;
use crate::ipc::{
    AiActivityListArgs, AiActivityListResult, AiProposeArgs, AiProposeReply, AiProposedToolCall,
    AiStreamAskMessage, AiStreamAskRole, AiStreamChatArgs, AiStreamChatMode, AiToolPolicy,
    AiUnmappedToolCall,
};
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;
use crate::provider::{AiProvider, ChatMessage, ChatTurn, ToolCall};
use crate::tools::{
    register_extended_builtins, register_storage_builtins, register_terminal_builtins,
    ToolError, ToolRegistry,
};
use crate::{rag, vectorstore};

/// Hard cap on tool-call rounds inside a single `stream_chat`
/// invocation. Each round = one provider call + N tool executions.
/// 8 is enough for realistic agent flows (read a file, search,
/// summarise, write) without letting a runaway loop pin the kernel.
const MAX_TOOL_ROUNDS: usize = 8;

/// Host-owned system-prompt floor (G3) applied to every
/// `mode=chat` call. Prepended to whatever `system` the caller
/// supplied — never replaces it. Skipped for `mode=complete` (the
/// ghost-completion contract is "raw completion text, no host
/// scaffolding"). Kept terse: every chat call carries this on the
/// wire, so token cost matters.
const HOST_SYSTEM_PROMPT_FLOOR: &str =
    "You are operating inside Nexus, the user's personal knowledge forge — \
     a directory of plain-text files (mostly Markdown). All file paths you \
     see or emit are forge-relative; never use absolute paths or paths \
     containing `..`. Prefer using available tools (reading, searching, \
     writing) over guessing. When you modify a file, make minimal targeted \
     edits and preserve the user's existing structure and tone.";

/// Names of read-only built-in tools shipped via
/// [`crate::tools::register_storage_builtins`] / `register_extended_builtins`
/// / `register_terminal_builtins`. Used by [`filter_to_read_only`]
/// to honour [`AiToolPolicy::AutoReadOnly`] (ADR 0022 Phase 2). Add
/// a tool here if a future built-in should be visible without
/// `ai.tools.write`. `terminal_get_status` is included because it
/// only reads session metadata; `terminal_run_saved` and
/// `terminal_send_signal` mutate process state and stay write-class.
const READ_ONLY_TOOL_NAMES: &[&str] = &[
    "read_file",
    "search_forge",
    "list_backlinks",
    "git_log",
    "terminal_get_status",
];

/// Build a fresh registry containing only the entries from
/// `source` whose names appear in [`READ_ONLY_TOOL_NAMES`]. Used for
/// `AiToolPolicy::AutoReadOnly`. Cloning the executors is cheap
/// (they're `Arc`-shared inside the registry).
fn filter_to_read_only(source: &ToolRegistry) -> ToolRegistry {
    let mut filtered = ToolRegistry::new();
    for schema in source.schemas() {
        if !READ_ONLY_TOOL_NAMES.contains(&schema.name.as_str()) {
            continue;
        }
        // The registry doesn't expose `RegisteredTool` lookups by
        // name — but `execute` does the dispatch we need, so we can
        // wrap the source registry behind a thin proxy. Simpler:
        // re-register from the executor table via a forwarding
        // executor.
        let name = schema.name.clone();
        let source_arc = source.clone();
        let exec: std::sync::Arc<dyn crate::tools::ToolExecutor> =
            std::sync::Arc::new(ForwardingExecutor {
                target_name: name.clone(),
                source: std::sync::Arc::new(source_arc),
            });
        filtered.register(name, schema, exec);
    }
    filtered
}

/// Forwards `execute` calls to a named tool in another registry —
/// used by [`filter_to_read_only`] so the filtered registry shares
/// the same executors as the source without exposing
/// `RegisteredTool` lookups on the registry's public surface.
struct ForwardingExecutor {
    target_name: String,
    source: std::sync::Arc<ToolRegistry>,
}

#[async_trait::async_trait]
impl crate::tools::ToolExecutor for ForwardingExecutor {
    async fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<String, crate::tools::ToolError> {
        self.source.execute(&self.target_name, input).await
    }
}

/// Compose the effective system prompt for `mode=chat`. Returns the
/// floor when `caller` is empty/`None`; returns `floor + "\n\n" +
/// caller` otherwise.
fn compose_chat_system(caller: Option<&str>) -> String {
    match caller.map(str::trim).filter(|s| !s.is_empty()) {
        Some(c) => format!("{HOST_SYSTEM_PROMPT_FLOOR}\n\n{c}"),
        None => HOST_SYSTEM_PROMPT_FLOOR.to_string(),
    }
}

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.ai";

/// Handler id for `ask` (RAG query).
pub const HANDLER_ASK: u32 = 1;
/// Handler id for `index_file` (chunk + embed + upsert).
pub const HANDLER_INDEX_FILE: u32 = 2;
/// Handler id for `vectorstore_count` (proxy to storage).
pub const HANDLER_VECTORSTORE_COUNT: u32 = 3;
/// Handler id for `status` (provider + indexed-chunk summary).
pub const HANDLER_STATUS: u32 = 4;
/// Handler id for `config` (detected provider snapshot — sync).
pub const HANDLER_CONFIG: u32 = 5;
/// Handler id for `stream_chat` (direct chat with per-token bus events).
///
/// `mode: chat` (default) uses the tool-dispatch loop;
/// `mode: complete` bypasses it for a single round-trip + post-processing
/// (used by ghost completion / headless `complete` CLI — BL-010/011/034).
pub const HANDLER_STREAM_CHAT: u32 = 6;
/// Handler id for `stream_ask` (RAG retrieve + streaming chat).
pub const HANDLER_STREAM_ASK: u32 = 7;
/// Handler id for `session_load` — read the persisted chat session
/// JSON from `<forge>/.forge/chat_session.json`. Returns `null` if
/// the file doesn't exist yet.
pub const HANDLER_SESSION_LOAD: u32 = 8;
/// Handler id for `session_save` — overwrite the persisted chat
/// session JSON. Args are an opaque JSON object; the plugin doesn't
/// inspect the shape.
pub const HANDLER_SESSION_SAVE: u32 = 9;
/// Handler id for `session_list` — enumerate multi-session files
/// under `<forge>/.forge/chat/sessions/`. Returns `[{ id, title?,
/// updated_at, bytes }]`.
pub const HANDLER_SESSION_LIST: u32 = 10;
/// Handler id for `session_delete` — remove a multi-session file by
/// id. Legacy single-session lives outside this tree and isn't
/// affected.
pub const HANDLER_SESSION_DELETE: u32 = 11;
/// Handler id for `set_config` — replace the in-memory `AiConfig` (and
/// optionally the embedding `AiConfig`) at runtime. Persistence lives
/// in the shell's config store; this handler only mutates the live
/// process so the next chat call uses the new credentials without a
/// restart. Args:
///
///   { ai?:        { provider, model?, `api_key`?, `base_url`? } | null,
///     embedding?: { provider, model?, `api_key`?, `base_url`? } | null }
///
/// A `null` clears that side; an absent key leaves it untouched.
pub const HANDLER_SET_CONFIG: u32 = 12;

/// BL-041 — `index_status`: snapshot of the background indexing
/// daemon's counters. Returns the [`crate::indexing_daemon::IndexStatus`]
/// shape: `{ indexed_files, pending_files, total_seen, last_error,
/// running }`. Cheap — pure read of the shared `Arc<RwLock<>>` state.
pub const HANDLER_INDEX_STATUS: u32 = 14;

/// BL-040 — `semantic_search`: embed a query and return the top-N
/// matching chunks from the vector store (no chat). Args
/// `{ query: String, limit?: usize (default 10) }`. Returns
/// `{ matches: Vec<ChunkMatch> }`.
pub const HANDLER_SEMANTIC_SEARCH: u32 = 13;

/// BL-045 — `enrich_file`: read a markdown file, run the AI provider
/// for tags + summary, run `semantic_search` for related notes, return
/// an [`crate::enrichment::EnrichmentProposal`] WITHOUT writing.
/// Args `{ path: String }` → JSON-serialised proposal.
pub const HANDLER_ENRICH_FILE: u32 = 15;

/// BL-045 — `enrich_apply`: merge a previously-returned
/// [`crate::enrichment::EnrichmentProposal`] back into the file's
/// YAML frontmatter, but only if `body_hash` still matches. Args
/// `{ proposal: EnrichmentProposal }` → `{ applied: bool, reason?:
/// String }`.
pub const HANDLER_ENRICH_APPLY: u32 = 16;

/// FU-2 (BL-041 follow-up) — `index_trigger`: walk every markdown
/// file currently known to `com.nexus.storage::query_files` and
/// enqueue it onto the indexing daemon's debouncer as a `Touched`.
/// No args. Returns `{ queued: usize }`. Idempotent — duplicate
/// pushes coalesce in the debouncer.
pub const HANDLER_INDEX_TRIGGER: u32 = 17;

/// BL-037 — `activity_list`: read the per-forge AI activity timeline
/// (newest-first). Args [`AiActivityListArgs`] (optionally caps the
/// number returned); result [`AiActivityListResult`].
pub const HANDLER_ACTIVITY_LIST: u32 = 18;

/// BL-037 — `activity_clear`: truncate the activity log to zero
/// bytes. No args, returns `{ cleared: true }`.
pub const HANDLER_ACTIVITY_CLEAR: u32 = 19;

/// G7 — `propose_tool_calls`: single-turn provider call that returns
/// the model's tool-use blocks WITHOUT executing them. Used by
/// `nexus-agent` (ADR 0023) to derive a `Plan` for later
/// approval-gated execution. Args [`AiProposeArgs`], reply
/// [`AiProposeReply`].
pub const HANDLER_PROPOSE_TOOL_CALLS: u32 = 20;

/// Core plugin for AI integration.
pub struct AiCorePlugin {
    /// Live config — wrapped in `Arc<RwLock<>>` so async handlers can
    /// hold cheap clones of the handle and pick up runtime updates
    /// pushed via [`HANDLER_SET_CONFIG`] without rebuilding the plugin.
    ai_config: Arc<RwLock<Option<AiConfig>>>,
    embed_config: Arc<RwLock<Option<AiConfig>>>,
    /// Plugin-facing kernel context, installed via [`CorePlugin::wire_context`]
    /// after the shared plugin loader + dispatcher are assembled. Handlers
    /// clone the `Arc` into their spawned futures. `None` if a handler fires
    /// before bootstrap finishes wiring — those handlers return a clear error.
    context: Option<Arc<KernelPluginContext>>,
    /// Tool registry the streaming dispatch loop offers to the model.
    /// Populated in [`CorePlugin::wire_context`] alongside `context` so
    /// the storage-backed `read_file` / `write_file` built-ins can route
    /// through `ipc_call`. Wrapped in `Arc` so handler futures get a
    /// cheap clone; the registry itself is read-only after bootstrap.
    tools: Option<Arc<ToolRegistry>>,
    /// BL-041 — shared snapshot of the background indexing daemon's
    /// counters. Allocated unconditionally (cheap) so the
    /// `HANDLER_INDEX_STATUS` IPC handler can return a meaningful
    /// "not yet running" view even before `on_start` runs and even
    /// when no embedding provider is configured (in which case the
    /// daemon thread spins but never flushes).
    index_status: SharedStatus,
    /// BL-041 — owning handle for the daemon thread. `None` until
    /// `on_start` runs; cleared in `on_stop` (which joins the worker).
    indexing_daemon: Option<IndexingDaemon>,
    /// FU-2 — clone of the daemon's input-channel sender, populated
    /// alongside `indexing_daemon`. Held in an `Arc<RwLock<>>` so the
    /// async `index_trigger` handler can reach it from a future
    /// without borrowing `&self`. Cleared on `on_stop`.
    daemon_tx: Arc<RwLock<Option<UnboundedSender<DaemonMsg>>>>,
    /// BL-037 — activity timeline recorder. `None` until
    /// `wire_context` runs; the `stream_chat` / `stream_ask` arms
    /// fire-and-forget through it on completion. Cloning the
    /// `ActivityRecorder` is cheap (it's a pair of `Arc`s).
    activity: Option<ActivityRecorder>,
}

impl AiCorePlugin {
    /// Construct an unstarted plugin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ai_config: Arc::new(RwLock::new(None)),
            embed_config: Arc::new(RwLock::new(None)),
            context: None,
            tools: None,
            index_status: indexing_daemon::new_status(),
            indexing_daemon: None,
            daemon_tx: Arc::new(RwLock::new(None)),
            activity: None,
        }
    }

    /// Return the detected AI chat-provider configuration, if any.
    /// Returned by clone since the lock is internal to the plugin.
    #[must_use]
    pub fn config(&self) -> Option<AiConfig> {
        self.ai_config.read().ok().and_then(|g| g.clone())
    }
}

impl Default for AiCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePlugin for AiCorePlugin {
    /// Detect AI and embedding providers from environment variables.
    /// The shell pushes user-saved config via [`HANDLER_SET_CONFIG`] on
    /// boot, so env detection is the floor — anything the user has set
    /// in Settings → AI overrides this once `set_config` lands.
    fn on_init(&mut self) -> Result<(), PluginError> {
        let ai = detect_provider();
        let embed = detect_embedding_provider();
        if let Some(cfg) = &ai {
            tracing::debug!(plugin_id = PLUGIN_ID, provider = %cfg.provider, "AI provider detected");
        } else {
            tracing::debug!(
                plugin_id = PLUGIN_ID,
                "no AI provider detected; AI features disabled"
            );
        }
        if let Ok(mut g) = self.ai_config.write() {
            *g = ai;
        }
        if let Ok(mut g) = self.embed_config.write() {
            *g = embed;
        }
        Ok(())
    }

    /// Sync dispatch only handles the one command that needs no I/O:
    /// `HANDLER_CONFIG`. Everything else is async — callers must route
    /// through [`CorePlugin::dispatch_async`].
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        if handler_id == HANDLER_CONFIG {
            let ai = self.ai_config.read().ok().and_then(|g| g.clone());
            let embed = self.embed_config.read().ok().and_then(|g| g.clone());
            return Ok(config_snapshot(ai.as_ref(), embed.as_ref()));
        }
        if handler_id == HANDLER_INDEX_STATUS {
            let snap = indexing_daemon::snapshot(&self.index_status);
            return serde_json::to_value(&snap)
                .map_err(|e| exec_err(format!("index_status: serialize: {e}")));
        }
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "handler {handler_id}: AI command is async; caller should use dispatch_async"
            ),
        })
    }

    /// Async dispatch path. Captures the context + configs into the returned
    /// future so nothing outlives the `&mut self` borrow.
    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        // Fall through to sync for the no-I/O handler so the caller can use
        // either path without surprises.
        if handler_id == HANDLER_CONFIG {
            let ai = self.ai_config.read().ok().and_then(|g| g.clone());
            let embed = self.embed_config.read().ok().and_then(|g| g.clone());
            let response = config_snapshot(ai.as_ref(), embed.as_ref());
            return Some(Box::pin(async move { Ok(response) }));
        }

        // BL-041 — `index_status` is a pure read of the shared status
        // handle; no I/O, but routed through `dispatch_async` for
        // shape symmetry with the rest of the AI surface (the shell
        // polls it via its standard `kernel_invoke` async path).
        if handler_id == HANDLER_INDEX_STATUS {
            let snap = indexing_daemon::snapshot(&self.index_status);
            let response = serde_json::to_value(&snap)
                .map_err(|e| exec_err(format!("index_status: serialize: {e}")));
            return Some(Box::pin(async move { response }));
        }

        // set_config: in-memory only, no I/O — but we model it as async
        // for symmetry with the rest of the surface and so the shell
        // can `await` confirmation that the new credentials are live
        // before emitting a "configured" UI event.
        if handler_id == HANDLER_SET_CONFIG {
            let ai_handle = Arc::clone(&self.ai_config);
            let embed_handle = Arc::clone(&self.embed_config);
            let args = args.clone();
            return Some(Box::pin(async move {
                handle_set_config(&ai_handle, &embed_handle, &args)
            }));
        }

        // FU-2 — `index_trigger`: enumerate forge markdown via storage
        // IPC and fan into the daemon's queue. Resolves the sender via
        // the cloned `daemon_tx` handle so the future can outlive the
        // dispatch borrow.
        if handler_id == HANDLER_INDEX_TRIGGER {
            let tx_handle = Arc::clone(&self.daemon_tx);
            let ctx_clone = self.context.clone();
            return Some(Box::pin(async move {
                let ctx = ctx_clone.ok_or_else(|| {
                    exec_err("AI plugin context not wired (bootstrap incomplete)")
                })?;
                handle_index_trigger(&ctx, &tx_handle).await
            }));
        }

        // BL-037 — `activity_list` / `activity_clear` go through the
        // recorder directly and don't need the kernel context. Routing
        // before the ctx-required arm lets the shell pane hydrate even
        // if the caller polls before `wire_context` runs (the recorder
        // returns an empty list in that case).
        if handler_id == HANDLER_ACTIVITY_LIST {
            let activity = self.activity.clone();
            let args = args.clone();
            return Some(Box::pin(async move {
                handle_activity_list(activity, &args).await
            }));
        }
        if handler_id == HANDLER_ACTIVITY_CLEAR {
            let activity = self.activity.clone();
            return Some(Box::pin(async move {
                handle_activity_clear(activity).await
            }));
        }

        let ctx = self.context.clone();
        let tools = self.tools.clone();
        let activity = self.activity.clone();
        let ai_cfg = self.ai_config.read().ok().and_then(|g| g.clone());
        let embed_cfg = self.embed_config.read().ok().and_then(|g| g.clone());
        let args = args.clone();

        Some(Box::pin(async move {
            let ctx =
                ctx.ok_or_else(|| exec_err("AI plugin context not wired (bootstrap incomplete)"))?;
            match handler_id {
                HANDLER_ASK => handle_ask(&ctx, ai_cfg, embed_cfg, &args).await,
                HANDLER_INDEX_FILE => handle_index_file(&ctx, embed_cfg, &args).await,
                HANDLER_VECTORSTORE_COUNT => handle_vectorstore_count(&ctx).await,
                HANDLER_STATUS => handle_status(&ctx, ai_cfg, embed_cfg).await,
                HANDLER_STREAM_CHAT => {
                    handle_stream_chat(ctx, ai_cfg, tools, activity, &args).await
                }
                HANDLER_STREAM_ASK => {
                    handle_stream_ask(ctx, ai_cfg, embed_cfg, activity, &args).await
                }
                HANDLER_SESSION_LOAD => handle_session_load(&ctx, &args).await,
                HANDLER_SESSION_SAVE => handle_session_save(&ctx, &args).await,
                HANDLER_SESSION_LIST => handle_session_list(&ctx).await,
                HANDLER_SESSION_DELETE => handle_session_delete(&ctx, &args).await,
                HANDLER_SEMANTIC_SEARCH => {
                    handle_semantic_search(&ctx, embed_cfg, &args).await
                }
                HANDLER_ENRICH_FILE => handle_enrich_file(&ctx, ai_cfg, embed_cfg, &args).await,
                HANDLER_ENRICH_APPLY => handle_enrich_apply(&ctx, &args).await,
                HANDLER_PROPOSE_TOOL_CALLS => {
                    handle_propose_tool_calls(ctx, ai_cfg, tools, &args).await
                }
                _ => Err(exec_err(format!("unknown handler id {handler_id}"))),
            }
        }))
    }

    /// Capture the kernel plugin context so async handlers can issue nested
    /// `ipc_call`s into storage through the canonical plugin surface.
    /// Also seeds the tool registry with the storage-backed built-ins
    /// (`read_file`, `write_file`) so the streaming dispatch loop has a
    /// non-empty toolbox without each frontend opting in.
    ///
    /// BL-041 — also spawns the background indexing daemon. We start
    /// it here (rather than `on_start`) because `wire_context` is the
    /// first lifecycle hook with the kernel context in hand; the
    /// daemon needs `ctx.subscribe(...)` and `ctx.ipc_call(...)`.
    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        let mut registry = ToolRegistry::new();
        register_storage_builtins(&mut registry, Arc::clone(&ctx));
        register_extended_builtins(&mut registry, Arc::clone(&ctx));
        register_terminal_builtins(&mut registry, Arc::clone(&ctx));
        self.tools = Some(Arc::new(registry));

        // BL-037 — bind the activity recorder to the same context so
        // every AI surface that flows through this plugin can record
        // an entry on completion without a per-handler ctx clone.
        self.activity = Some(ActivityRecorder::new(Arc::clone(&ctx)));

        // Build the embedder factory — captures the live `embed_config`
        // handle so the daemon picks up runtime `set_config` updates
        // without a restart. Returns `None` when no embedding provider
        // is configured; the daemon then logs `last_error` and skips
        // the touch path until the user sets one.
        let embed_handle = Arc::clone(&self.embed_config);
        let factory: EmbedderFactory = Arc::new(move || {
            let cfg = embed_handle.read().ok().and_then(|g| g.clone())?;
            build_embedding_provider(&cfg).ok()
        });

        match IndexingDaemon::start(Arc::clone(&ctx), Arc::clone(&self.index_status), factory) {
            Ok(daemon) => {
                if let Ok(mut g) = self.daemon_tx.write() {
                    *g = daemon.sender_handle();
                }
                self.indexing_daemon = Some(daemon);
                tracing::debug!(plugin_id = PLUGIN_ID, "BL-041 indexing daemon started");
            }
            Err(e) => {
                tracing::error!(
                    plugin_id = PLUGIN_ID,
                    ?e,
                    "BL-041 indexing daemon failed to start"
                );
                if let Ok(mut g) = self.index_status.write() {
                    g.last_error = Some(format!("daemon spawn failed: {e}"));
                }
            }
        }

        self.context = Some(ctx);
    }

    /// BL-041 — gracefully stop the indexing daemon on shutdown.
    fn on_stop(&mut self) {
        if let Ok(mut g) = self.daemon_tx.write() {
            *g = None;
        }
        if let Some(mut d) = self.indexing_daemon.take() {
            d.stop();
        }
    }
}

// ─── Handler implementations ────────────────────────────────────────────────

async fn handle_ask(
    ctx: &KernelPluginContext,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let question = args
        .get("question")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("ask: missing 'question' string"))?;
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(5);

    let ai_cfg = ai_cfg.ok_or_else(|| exec_err("ask: no AI chat provider configured"))?;
    let embed_cfg =
        embed_cfg.ok_or_else(|| exec_err("ask: no AI embedding provider configured"))?;

    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let response = rag::query(ctx, ai.as_ref(), embedder.as_ref(), question, limit)
        .await
        .map_err(|e| exec_err(format!("rag query failed: {e}")))?;
    serde_json::to_value(&response).map_err(|e| exec_err(format!("ask: serialize: {e}")))
}

/// BL-040 — embed `query` and return the top-`limit` chunks from the
/// vector store (no chat). Mirrors the embedder build path of
/// [`handle_ask`] but skips the chat provider entirely so callers
/// (palette, TUI, MCP) get a fast, score-bearing list of hits.
async fn handle_semantic_search(
    ctx: &KernelPluginContext,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let query = args
        .get("query")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("semantic_search: missing 'query' string"))?;
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(10);

    let embed_cfg = embed_cfg
        .ok_or_else(|| exec_err("semantic_search: no AI embedding provider configured"))?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let matches = rag::semantic_search(ctx, embedder.as_ref(), query, limit)
        .await
        .map_err(|e| exec_err(format!("semantic_search: {e}")))?;
    Ok(serde_json::json!({ "matches": matches }))
}

/// BL-045 — `enrich_file`: read a markdown note, ask the AI for
/// tags + summary, run `semantic_search` for related notes, return
/// an [`crate::enrichment::EnrichmentProposal`] WITHOUT writing.
async fn handle_enrich_file(
    ctx: &KernelPluginContext,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let path = args
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("enrich_file: missing 'path' string"))?;

    let ai_cfg = ai_cfg.ok_or_else(|| exec_err("enrich_file: no AI chat provider configured"))?;
    let embed_cfg =
        embed_cfg.ok_or_else(|| exec_err("enrich_file: no AI embedding provider configured"))?;

    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let proposal = crate::enrichment::propose(ctx, ai.as_ref(), embedder.as_ref(), path)
        .await
        .map_err(|e| exec_err(format!("enrich_file: {e}")))?;
    serde_json::to_value(&proposal)
        .map_err(|e| exec_err(format!("enrich_file: serialize: {e}")))
}

/// BL-045 — `enrich_apply`: merge a previously-returned proposal back
/// into the file's YAML frontmatter, but only if `body_hash` still
/// matches. Returns `{ applied: bool, reason?: String }`.
async fn handle_enrich_apply(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let raw_proposal = args
        .get("proposal")
        .ok_or_else(|| exec_err("enrich_apply: missing 'proposal'"))?;
    let proposal: crate::enrichment::EnrichmentProposal =
        serde_json::from_value(raw_proposal.clone())
            .map_err(|e| exec_err(format!("enrich_apply: proposal decode: {e}")))?;
    let (applied, reason) = crate::enrichment::apply(ctx, &proposal)
        .await
        .map_err(|e| exec_err(format!("enrich_apply: {e}")))?;
    Ok(serde_json::json!({
        "applied": applied,
        "reason": reason,
    }))
}

/// FU-2 — fan every markdown file in the storage index into the
/// indexing daemon as a `Touched`. Returns `{ queued: usize }`.
///
/// We go through `com.nexus.storage::query_files` rather than walking
/// the filesystem because storage already owns the canonical
/// inventory of forge files (and respects deletions, the
/// `.forge/` quarantine, etc.). Files not yet known to storage will
/// be picked up by the next file-watcher event — the indexing
/// daemon's debouncer dedupes overlap.
async fn handle_index_trigger(
    ctx: &KernelPluginContext,
    daemon_tx: &Arc<RwLock<Option<UnboundedSender<DaemonMsg>>>>,
) -> Result<serde_json::Value, PluginError> {
    let tx = daemon_tx
        .read()
        .ok()
        .and_then(|g| g.clone())
        .ok_or_else(|| exec_err("index_trigger: indexing daemon not running"))?;

    let response = ctx
        .ipc_call(
            "com.nexus.storage",
            "query_files",
            serde_json::json!({}),
            std::time::Duration::from_secs(30),
        )
        .await
        .map_err(|e| exec_err(format!("index_trigger: query_files: {e}")))?;

    let records = response
        .as_array()
        .ok_or_else(|| exec_err("index_trigger: query_files returned non-array"))?;

    let mut queued: usize = 0;
    for entry in records {
        let Some(path) = entry.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let ext_ok = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
        if !ext_ok {
            continue;
        }
        if tx.send(DaemonMsg::Touched(std::path::PathBuf::from(path))).is_ok() {
            queued += 1;
        }
    }

    tracing::debug!(plugin_id = PLUGIN_ID, queued, "index_trigger fanned forge");
    Ok(serde_json::json!({ "queued": queued }))
}

async fn handle_index_file(
    ctx: &KernelPluginContext,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let file_path = args
        .get("file_path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("index_file: missing 'file_path' string"))?;
    let blocks: Vec<(u64, String, String, Option<i32>)> = args
        .get("blocks")
        .ok_or_else(|| exec_err("index_file: missing 'blocks'"))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("index_file: blocks decode: {e}")))
        })?;

    let embed_cfg =
        embed_cfg.ok_or_else(|| exec_err("index_file: no AI embedding provider configured"))?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let count = rag::index_file(ctx, embedder.as_ref(), file_path, &blocks)
        .await
        .map_err(|e| exec_err(format!("index_file: {e}")))?;
    Ok(serde_json::json!({ "indexed_chunks": count }))
}

async fn handle_vectorstore_count(
    ctx: &KernelPluginContext,
) -> Result<serde_json::Value, PluginError> {
    let count = vectorstore::count(ctx)
        .await
        .map_err(|e| exec_err(format!("vectorstore_count: {e}")))?;
    Ok(serde_json::json!({ "count": count }))
}

async fn handle_status(
    ctx: &KernelPluginContext,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
) -> Result<serde_json::Value, PluginError> {
    let count = vectorstore::count(ctx)
        .await
        .map_err(|e| exec_err(format!("status: vectorstore_count: {e}")))?;
    let embedding_model = embed_cfg.as_ref().and_then(resolve_embedding_model);
    let embedding_dimension = embed_cfg.as_ref().and_then(resolve_embedding_dimension);
    let tls_pinned = tls_pinning_effective(ai_cfg.as_ref());
    Ok(serde_json::json!({
        "ai_provider": ai_cfg.as_ref().map(|c| c.provider.clone()),
        "ai_model": ai_cfg.as_ref().and_then(|c| c.model.clone()),
        "embedding_provider": embed_cfg.as_ref().map(|c| c.provider.clone()),
        "embedding_model": embedding_model,
        "embedding_dimension": embedding_dimension,
        "indexed_chunks": count,
        "tls_pinned": tls_pinned,
    }))
}

/// BL-102 follow-up — mirrors the gate in
/// [`crate::http_client::build_client`] so the wire field tracks the
/// HTTP client that actually got built. Pinning is on iff the AI
/// config flag is set OR `NEXUS_TLS_PINNING=1` is in the environment.
fn tls_pinning_effective(ai_cfg: Option<&AiConfig>) -> bool {
    let cfg_flag = ai_cfg.is_some_and(|c| c.tls_pinning_enabled);
    let env_opt_in = std::env::var("NEXUS_TLS_PINNING")
        .map(|v| v == "1")
        .unwrap_or(false);
    cfg_flag || env_opt_in
}

/// AIG-05 — resolve the embedding model identifier for status
/// reporting. For `provider = "local"` we prefer the
/// `local_embedding_model` slot (the canonical place the local
/// backend reads from); other providers fall back to the chat-style
/// `model` field.
fn resolve_embedding_model(cfg: &AiConfig) -> Option<String> {
    if cfg.provider == "local" {
        return cfg.local_embedding_model.clone();
    }
    cfg.model.clone()
}

/// AIG-05 — embedding-vector dimension when the local backend is the
/// configured provider AND the `local-embeddings` feature is built
/// in. Returns `None` for remote providers (the wire shape isn't
/// universally exposed) and for unrecognised local model
/// identifiers.
#[cfg(feature = "local-embeddings")]
fn resolve_embedding_dimension(cfg: &AiConfig) -> Option<usize> {
    if cfg.provider != "local" {
        return None;
    }
    let id = cfg.local_embedding_model.as_deref().unwrap_or("");
    crate::local_embedding::dimension_for(id)
}

#[cfg(not(feature = "local-embeddings"))]
fn resolve_embedding_dimension(_cfg: &AiConfig) -> Option<usize> {
    None
}

/// Live-update the in-memory `AiConfig` for chat and/or embedding.
///
/// Args shape:
///
/// ```json
/// {
///   "ai":        { "provider": "anthropic", "api_key": "...", "model": null, "base_url": null } | null,
///   "embedding": { "provider": "openai",    "api_key": "...", "model": null, "base_url": null } | null
/// }
/// ```
///
/// Field-level rules:
///   - `provider` is required when the side is present and non-null.
///     An empty string clears that side (same as passing `null`).
///   - `api_key` / `model` / `base_url` are optional; absent → `None`.
///   - An absent top-level key (no `"ai"` field at all) leaves that
///     side untouched.
///
/// The shell pushes this on every boot from its persisted config
/// store, so a user who set `provider=ollama` once gets it back on
/// the next launch without re-typing.
fn handle_set_config(
    ai_handle: &Arc<RwLock<Option<AiConfig>>>,
    embed_handle: &Arc<RwLock<Option<AiConfig>>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let obj = args
        .as_object()
        .ok_or_else(|| exec_err("set_config: expected JSON object"))?;

    if obj.contains_key("ai") {
        let next = parse_config_field(obj.get("ai").unwrap_or(&serde_json::Value::Null))?;
        let mut g = ai_handle
            .write()
            .map_err(|_| exec_err("set_config: ai config lock poisoned"))?;
        *g = next;
    }
    if obj.contains_key("embedding") {
        let next = parse_config_field(obj.get("embedding").unwrap_or(&serde_json::Value::Null))?;
        let mut g = embed_handle
            .write()
            .map_err(|_| exec_err("set_config: embedding config lock poisoned"))?;
        *g = next;
    }

    let ai_view = ai_handle.read().ok().and_then(|g| g.clone());
    let embed_view = embed_handle.read().ok().and_then(|g| g.clone());
    Ok(config_snapshot(ai_view.as_ref(), embed_view.as_ref()))
}

/// Decode one side of the `set_config` payload. `Null` and a missing /
/// empty `provider` both mean "clear this side" — that's the path the
/// shell uses when the user blanks out the provider dropdown in
/// Settings → AI.
fn parse_config_field(value: &serde_json::Value) -> Result<Option<AiConfig>, PluginError> {
    if value.is_null() {
        return Ok(None);
    }
    let obj = value
        .as_object()
        .ok_or_else(|| exec_err("set_config: provider config must be object or null"))?;
    let provider = obj
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if provider.is_empty() {
        return Ok(None);
    }
    let model = obj
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let api_key = obj
        .get("api_key")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let base_url = obj
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    // AIG-05 — `provider = "local"` lifts the `model` field into
    // `local_embedding_model`, the slot
    // `crate::config::detect_local_embedding` populates from
    // `NEXUS_LOCAL_EMBEDDING_MODEL`. Empty string and a missing key
    // both fall back to the canonical default
    // (`crate::local_embedding::DEFAULT_LOCAL_MODEL`).
    let local_embedding_model = if provider == "local" {
        model.clone()
    } else {
        None
    };
    Ok(Some(AiConfig {
        provider,
        model,
        api_key,
        base_url,
        local_embedding_model,
        ..AiConfig::default()
    }))
}

/// Build a detected-provider snapshot (synchronous — no I/O).
fn config_snapshot(ai_cfg: Option<&AiConfig>, embed_cfg: Option<&AiConfig>) -> serde_json::Value {
    #[derive(Serialize)]
    struct ConfigView<'a> {
        provider: &'a str,
        model: Option<&'a str>,
        base_url: Option<&'a str>,
        has_api_key: bool,
        /// AIG-05 — populated only for `provider = "local"` so the
        /// shell can render the resolved local-embedding identifier
        /// without inferring it from the chat-style `model` field.
        #[serde(skip_serializing_if = "Option::is_none")]
        local_embedding_model: Option<&'a str>,
    }
    fn view(cfg: &AiConfig) -> ConfigView<'_> {
        ConfigView {
            provider: cfg.provider.as_str(),
            model: cfg.model.as_deref(),
            base_url: cfg.base_url.as_deref(),
            has_api_key: cfg.api_key.is_some(),
            local_embedding_model: if cfg.provider == "local" {
                cfg.local_embedding_model.as_deref()
            } else {
                None
            },
        }
    }
    serde_json::json!({
        "ai": ai_cfg.map(view),
        "embedding": embed_cfg.map(view),
    })
}

// Each error path records an `ActivityEntry` so the timeline shows
// failures alongside successes — the overall control flow is mostly
// linear but needs a recording line at every exit. Splitting per
// branch would just push the same record_activity_error call out to
// callers.
#[allow(clippy::too_many_lines, reason = "BL-037 records on every exit path; flow stays linear")]
/// G7 — single-turn provider call that returns the model's tool-use
/// blocks without executing any of them, for the agent's
/// plan-then-approve flow (ADR 0023).
///
/// Mirrors `stream_chat`'s setup (registry resolution per
/// `AiToolPolicy`, including the MCP bridge under `AutoWithMcp`)
/// but uses `chat_turn_with_tools` exactly once with a no-op chunk
/// sink. Streaming events are intentionally NOT published — this
/// handler is for backgrounded planning, not user-visible chat.
async fn handle_propose_tool_calls(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    tools: Option<Arc<ToolRegistry>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiProposeArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("propose_tool_calls: args decode: {e}")))?;

    let ai_cfg = ai_cfg
        .ok_or_else(|| exec_err("propose_tool_calls: no AI chat provider configured"))?;
    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;

    let policy = parsed.tools.unwrap_or_default();
    let registry: Arc<ToolRegistry> = match policy {
        AiToolPolicy::None => Arc::new(ToolRegistry::new()),
        AiToolPolicy::Auto => tools.unwrap_or_else(|| Arc::new(ToolRegistry::new())),
        AiToolPolicy::AutoWithMcp => {
            let base = tools.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
            crate::tools::discover_mcp_tools(Arc::clone(&ctx), base).await
        }
        AiToolPolicy::AutoReadOnly => {
            let base = tools.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
            Arc::new(filter_to_read_only(&base))
        }
    };

    let messages = ipc_messages_to_chat(&parsed.messages);
    let turns = messages_to_turns(messages);
    let schemas = registry.schemas();
    let on_chunk = |_: String| {};
    let output = ai
        .chat_turn_with_tools(&turns, parsed.system.as_deref(), &schemas, &on_chunk)
        .await
        .map_err(|e| exec_err(format!("propose_tool_calls: provider: {e}")))?;

    let mut mapped: Vec<AiProposedToolCall> = Vec::new();
    let mut unmapped: Vec<AiUnmappedToolCall> = Vec::new();
    for call in output.tool_calls {
        match crate::tools::dispatch_target(&call.name, call.input.clone()) {
            Ok(target) => mapped.push(AiProposedToolCall {
                id: call.id,
                name: call.name,
                target_plugin_id: target.target_plugin_id,
                command_id: target.command_id,
                args: target.args,
            }),
            Err(e) => unmapped.push(AiUnmappedToolCall {
                id: call.id,
                name: call.name,
                input: call.input,
                error: e.to_string(),
            }),
        }
    }

    let reply = AiProposeReply {
        text: output.text,
        tool_calls: mapped,
        unmapped_tool_calls: unmapped,
    };
    serde_json::to_value(&reply)
        .map_err(|e| exec_err(format!("propose_tool_calls: encode reply: {e}")))
}

async fn handle_stream_chat(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    tools: Option<Arc<ToolRegistry>>,
    activity: Option<ActivityRecorder>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    // Decode through the typed `AiStreamChatArgs`. The wire shape matches
    // the historical ad-hoc `{ messages, system, session_id }` shape
    // exactly (same field names, same `lowercase` role tags), so existing
    // chat callers keep working without modification — only the new
    // optional fields (`mode`, `tools`, `max_tokens`, `stop`, `trim`,
    // `surface`) change behaviour for BL-010 / BL-011 / BL-034 / BL-037
    // callers.
    let parsed: AiStreamChatArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("stream_chat: args decode: {e}")))?;
    let messages = ipc_messages_to_chat(&parsed.messages);
    let system = parsed.system.clone();
    let session_id = parsed
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Capture pre-flight metadata for the activity entry. We resolve
    // these even on the error path so a failed call still gets logged
    // with prompt + provider + surface.
    let started_at = std::time::Instant::now();
    let prompt_text = last_user_prompt(&parsed.messages);
    let mode = parsed.mode.unwrap_or_default();
    let surface = resolve_surface(parsed.surface.as_deref(), mode);
    let provider_label = ai_cfg.as_ref().map(|c| c.provider.clone());
    let model_label = ai_cfg.as_ref().and_then(|c| c.model.clone());

    let Some(ai_cfg) = ai_cfg else {
        let err = "stream_chat: no AI chat provider configured";
        record_activity_error(
            activity.as_ref(),
            &session_id,
            surface,
            provider_label.clone(),
            model_label.clone(),
            prompt_text.clone(),
            started_at,
            err,
        )
        .await;
        return Err(exec_err(err));
    };

    // mode=complete forces tools=none regardless of the caller's value
    // — the contract is "single round-trip, no side effects".
    let tool_policy = match mode {
        AiStreamChatMode::Complete => AiToolPolicy::None,
        AiStreamChatMode::Chat => parsed.tools.unwrap_or_default(),
    };

    let envelope = EngineEnvelope::new(Arc::clone(&ctx), session_id.clone());
    envelope.publish_start();

    let outcome = match mode {
        AiStreamChatMode::Chat => {
            let registry = tools.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
            let registry_for_loop: Arc<ToolRegistry> = match tool_policy {
                AiToolPolicy::Auto => registry,
                // Physically empty registry so the loop advertises no
                // schemas and ignores any (impossible) tool_calls.
                AiToolPolicy::None => Arc::new(ToolRegistry::new()),
                // Discover MCP-advertised tools per call and merge with
                // built-ins. Discovery failures degrade gracefully —
                // discover_mcp_tools always returns a usable registry.
                AiToolPolicy::AutoWithMcp => {
                    crate::tools::discover_mcp_tools(Arc::clone(&ctx), registry).await
                }
                // ADR 0022 Phase 2 — read-only subset. Filter is by
                // tool name; runtime executors are shared with the
                // unfiltered registry.
                AiToolPolicy::AutoReadOnly => Arc::new(filter_to_read_only(&registry)),
            };
            let on_chunk = envelope.chunk_sink();
            let ai = match build_ai_provider(&ai_cfg) {
                Ok(p) => p,
                Err(e) => {
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &e,
                    )
                    .await;
                    return Err(exec_err(e));
                }
            };
            let effective_system = compose_chat_system(system.as_deref());
            match run_tool_dispatch_loop(
                ai.as_ref(),
                registry_for_loop.as_ref(),
                messages,
                Some(effective_system.as_str()),
                &on_chunk,
            )
            .await
            {
                Ok(o) => o,
                Err(e) => {
                    let msg = format!("stream_chat: {e}");
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &msg,
                    )
                    .await;
                    return Err(exec_err(msg));
                }
            }
        }
        AiStreamChatMode::Complete => {
            let ai = match build_ai_provider(&ai_cfg) {
                Ok(p) => p,
                Err(e) => {
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &e,
                    )
                    .await;
                    return Err(exec_err(e));
                }
            };
            let on_chunk = envelope.chunk_sink();
            let text = match run_complete(
                ai.as_ref(),
                &messages,
                system.as_deref(),
                &parsed,
                &on_chunk,
            )
            .await
            {
                Ok(t) => t,
                Err(e) => {
                    let msg = format!("stream_chat: {e}");
                    record_activity_error(
                        activity.as_ref(),
                        &session_id,
                        surface,
                        provider_label.clone(),
                        model_label.clone(),
                        prompt_text.clone(),
                        started_at,
                        &msg,
                    )
                    .await;
                    return Err(exec_err(msg));
                }
            };
            // mode=complete has no tool execution.
            ToolDispatchOutcome {
                text,
                tool_calls: Vec::new(),
                files: Vec::new(),
            }
        }
    };

    envelope.publish_done(&outcome.text);

    if let Some(rec) = activity {
        let entry = ActivityEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            surface,
            origin: "ai".into(),
            provider: provider_label,
            model: model_label,
            prompt: prompt_text,
            files: outcome.files.clone(),
            tool_calls: outcome.tool_calls.clone(),
            outcome: ActivityOutcome::Ok,
            error: None,
            duration_ms: u64::try_from(started_at.elapsed().as_millis()).ok(),
        };
        rec.append(entry).await;
    }

    Ok(serde_json::json!({"session_id": session_id, "text": outcome.text}))
}

/// Default surface tag derivation when the caller doesn't supply one.
/// `mode=complete` defaults to `Complete`; everything else defaults
/// to `Chat`.
fn resolve_surface(explicit: Option<&str>, mode: AiStreamChatMode) -> ActivitySurface {
    if let Some(s) = explicit {
        return ActivitySurface::from_str_lossy(s);
    }
    match mode {
        AiStreamChatMode::Complete => ActivitySurface::Complete,
        AiStreamChatMode::Chat => ActivitySurface::Chat,
    }
}

/// Extract the most recent user message's content as the prompt
/// recorded on the timeline. Empty when no user turn is present.
fn last_user_prompt(messages: &[AiStreamAskMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, AiStreamAskRole::User))
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
async fn record_activity_error(
    rec: Option<&ActivityRecorder>,
    session_id: &str,
    surface: ActivitySurface,
    provider: Option<String>,
    model: Option<String>,
    prompt: String,
    started_at: std::time::Instant,
    error: &str,
) {
    let Some(rec) = rec else { return };
    let entry = ActivityEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        session_id: session_id.to_string(),
        surface,
        origin: "ai".into(),
        provider,
        model,
        prompt,
        files: Vec::new(),
        tool_calls: Vec::new(),
        outcome: ActivityOutcome::Error,
        error: Some(error.to_string()),
        duration_ms: u64::try_from(started_at.elapsed().as_millis()).ok(),
    };
    rec.append(entry).await;
}

/// Translate the typed IPC message list into the provider-facing
/// [`ChatMessage`] shape. A pure projection: same fields, same role
/// names — both serialize as `lowercase`.
fn ipc_messages_to_chat(messages: &[AiStreamAskMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|m| ChatMessage {
            role: match m.role {
                AiStreamAskRole::System => crate::provider::Role::System,
                AiStreamAskRole::User => crate::provider::Role::User,
                AiStreamAskRole::Assistant => crate::provider::Role::Assistant,
            },
            content: m.content.clone(),
        })
        .collect()
}

/// Shared bus contract for the `stream_chat` family. Owns the
/// `stream_start` / `stream_chunk` / `stream_done` publishes so the
/// chat-loop and complete paths produce byte-identical event streams
/// (modulo content). Any future surface that needs to publish on the
/// same channels must go through this helper.
struct EngineEnvelope {
    ctx: Arc<KernelPluginContext>,
    session_id: String,
    chunk_idx: Arc<AtomicUsize>,
}

impl EngineEnvelope {
    fn new(ctx: Arc<KernelPluginContext>, session_id: String) -> Self {
        Self {
            ctx,
            session_id,
            chunk_idx: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn publish_start(&self) {
        let _ = self.ctx.publish(
            "com.nexus.ai.stream_start",
            serde_json::json!({"session_id": &self.session_id}),
        );
    }

    /// Build the per-token sink. The closure is `Send + Sync` so it
    /// can cross provider HTTP boundaries (Anthropic / `OpenAI` adapters
    /// invoke it from inside their streaming task).
    fn chunk_sink(&self) -> impl Fn(String) + Send + Sync + 'static {
        let ctx = Arc::clone(&self.ctx);
        let sid = self.session_id.clone();
        let idx = Arc::clone(&self.chunk_idx);
        move |chunk: String| {
            let i = idx.fetch_add(1, Ordering::Relaxed);
            let _ = ctx.publish(
                "com.nexus.ai.stream_chunk",
                serde_json::json!({
                    "session_id": &sid,
                    "chunk": chunk,
                    "index": i,
                }),
            );
        }
    }

    fn publish_done(&self, text: &str) {
        let _ = self.ctx.publish(
            "com.nexus.ai.stream_done",
            serde_json::json!({"session_id": &self.session_id, "text": text}),
        );
    }
}

/// `mode=complete` path: single provider round-trip with `chat_stream_with`,
/// no tools advertised, optional post-processing applied. The chat
/// dispatch loop is bypassed entirely — there is no second round, so
/// the model physically cannot trigger side effects even if the
/// provider misbehaves.
///
/// Provider + chunk sink are passed in (rather than built from
/// `AiConfig` + `EngineEnvelope`) so tests can drive this function
/// with `ScriptedProvider` and a recording sink without standing up
/// a kernel context. The handler is responsible for envelope
/// `stream_start` / `stream_done` framing — this function only owns
/// the provider call and post-processing.
async fn run_complete(
    ai: &dyn AiProvider,
    messages: &[ChatMessage],
    system: Option<&str>,
    args: &AiStreamChatArgs,
    on_chunk: &(dyn Fn(String) + Send + Sync),
) -> Result<String, String> {
    let raw = ai
        .chat_stream_with(messages, system, on_chunk)
        .await
        .map_err(|e| e.to_string())?;

    // Post-processing — all string-level, all keyed off the typed args.
    // We don't mutate what already streamed via `on_chunk`; only the
    // returned text + the `stream_done` payload reflect the trim.
    let mut text: String = raw;
    if args.trim == Some(true) {
        // Strip any leading echo of the prompt's tail. The "tail" is
        // the trailing 256 chars of the last user message — long
        // enough to catch typical paragraph echoes, short enough that
        // a coincidental prefix collision is unlikely.
        let prompt_tail = last_user_tail(messages, 256);
        let stripped = strip_prompt_echo(&text, &prompt_tail).to_string();
        // Then clip to the last natural break inside whatever's left.
        let clipped = trim_to_natural_break(&stripped).to_string();
        text = clipped;
    }
    if let Some(stops) = args.stop.as_deref() {
        // Stops apply unconditionally when set — a stop is a hard
        // contract, not a "nice to have". They run after `trim` so
        // a stop sequence inside the trimmed natural-break window
        // still wins.
        text = apply_stop(&text, stops).to_string();
    }
    Ok(text)
}

/// Last `n` chars (by char count, not bytes) of the most recent user
/// message — the heuristic for prompt-echo detection in
/// [`strip_prompt_echo`]. Returns `""` if there is no user message.
fn last_user_tail(messages: &[ChatMessage], n: usize) -> String {
    let Some(last_user) = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, crate::provider::Role::User))
    else {
        return String::new();
    };
    let chars: Vec<char> = last_user.content.chars().collect();
    let start = chars.len().saturating_sub(n);
    chars[start..].iter().collect()
}

/// If `suggested` begins with a copy of `prompt_tail` (the trailing
/// chars of the user's prompt), strip it. Models — especially
/// instruction-tuned ones — often echo the prompt before continuing;
/// callers want only the continuation. Whitespace at the seam is
/// trimmed too so the caller doesn't have to handle "\n" / " " glue
/// artifacts. No-op if `prompt_tail` is empty or doesn't match.
fn strip_prompt_echo<'a>(suggested: &'a str, prompt_tail: &str) -> &'a str {
    if prompt_tail.is_empty() {
        return suggested;
    }
    // Try the full tail first, then progressively shorter suffixes —
    // models sometimes echo only the last sentence/word of the
    // prompt, not the whole tail.
    let tail_chars: Vec<char> = prompt_tail.chars().collect();
    for start in 0..tail_chars.len() {
        let candidate: String = tail_chars[start..].iter().collect();
        if candidate.is_empty() {
            break;
        }
        if let Some(rest) = suggested.strip_prefix(candidate.as_str()) {
            return rest.trim_start();
        }
    }
    suggested
}

/// Clip `text` at its last natural break — defined as the position
/// right after the last sentence-ending punctuation (`.`, `!`, `?`)
/// or, failing that, the last newline. If neither exists the input
/// is returned unchanged. Trailing whitespace on the kept slice is
/// trimmed.
///
/// Used by ghost completion to avoid mid-sentence cliffs when the
/// model overruns the natural stopping point.
fn trim_to_natural_break(text: &str) -> &str {
    if text.is_empty() {
        return text;
    }
    // Walk from the end; first sentence terminator wins. We compare
    // on byte indices so we don't slice through a multi-byte char —
    // `char_indices` gives us the byte index of each char start.
    let mut sentence_end: Option<usize> = None;
    let mut newline_end: Option<usize> = None;
    for (i, c) in text.char_indices() {
        let next = i + c.len_utf8();
        match c {
            '.' | '!' | '?' => sentence_end = Some(next),
            '\n' => newline_end = Some(next),
            _ => {}
        }
    }
    let cut = sentence_end.or(newline_end);
    match cut {
        Some(end) => text[..end].trim_end(),
        None => text,
    }
}

/// Truncate `text` at the earliest occurrence of any string in
/// `stops`. The stop sequence itself is dropped; everything before
/// it is kept verbatim (no whitespace trim — the caller asked for
/// an exact cut). If no stop matches, return `text` unchanged.
///
/// Implemented as `O(text * stops.len())` which is fine: typical stop
/// lists are 1–4 short strings.
fn apply_stop<'a>(text: &'a str, stops: &[String]) -> &'a str {
    let mut earliest: Option<usize> = None;
    for s in stops {
        if s.is_empty() {
            continue;
        }
        if let Some(pos) = text.find(s.as_str()) {
            earliest = Some(match earliest {
                Some(prev) => prev.min(pos),
                None => pos,
            });
        }
    }
    match earliest {
        Some(pos) => &text[..pos],
        None => text,
    }
}

/// Tool-aware streaming dispatch loop. Builds an initial set of
/// [`ChatTurn`]s from the incoming `messages` array, calls the
/// provider, executes any tool calls the model requested through the
/// registry, and re-calls the provider until the model returns no more
/// tool calls — or [`MAX_TOOL_ROUNDS`] is hit.
///
/// `on_chunk` is forwarded to the provider on every iteration so the
/// UI sees text deltas across all rounds in order.
///
/// Returns the concatenated text from every assistant turn (so
/// downstream `stream_done` consumers see the full reasoning trail,
/// not just the final summary).
/// Outcome of [`run_tool_dispatch_loop`]: aggregated text + the
/// per-call recording the BL-037 activity timeline needs (tool name,
/// ok/error, file paths the model touched). Files are extracted from
/// the well-known `path` input field used by `read_file` /
/// `write_file` and similar; tools without a `path` arg contribute
/// nothing to `files`.
#[derive(Debug)]
struct ToolDispatchOutcome {
    text: String,
    tool_calls: Vec<nexus_types::activity::ActivityToolCall>,
    files: Vec<String>,
}

async fn run_tool_dispatch_loop(
    ai: &dyn AiProvider,
    registry: &ToolRegistry,
    messages: Vec<crate::provider::ChatMessage>,
    system: Option<&str>,
    on_chunk: &(dyn Fn(String) + Send + Sync),
) -> Result<ToolDispatchOutcome, String> {
    let mut turns = messages_to_turns(messages);
    let schemas = registry.schemas();

    let mut aggregated = String::new();
    let mut round: usize = 0;
    let mut tool_calls_recorded: Vec<nexus_types::activity::ActivityToolCall> = Vec::new();
    let mut files_recorded: Vec<String> = Vec::new();

    loop {
        round += 1;
        let output = ai
            .chat_turn_with_tools(&turns, system, &schemas, on_chunk)
            .await
            .map_err(|e| e.to_string())?;

        if !output.text.is_empty() {
            if !aggregated.is_empty() {
                aggregated.push('\n');
            }
            aggregated.push_str(&output.text);
        }

        if output.tool_calls.is_empty() {
            // Model is done. Whether it produced text this round or
            // not, we've reached steady state.
            return Ok(ToolDispatchOutcome {
                text: aggregated,
                tool_calls: tool_calls_recorded,
                files: files_recorded,
            });
        }

        // Append the assistant's tool-use turn so the next provider
        // call sees the conversation history correctly.
        turns.push(ChatTurn::Assistant {
            content: output.text.clone(),
            tool_calls: output.tool_calls.clone(),
        });

        // Execute every tool the model asked for, in order, and
        // append a ToolResult turn for each.
        for call in &output.tool_calls {
            let (content, is_error) = execute_tool_call(registry, call).await;
            tool_calls_recorded.push(nexus_types::activity::ActivityToolCall {
                name: call.name.clone(),
                ok: !is_error,
            });
            // Extract a file path if the tool input carried one. Most
            // file-touching tools (read_file, write_file, note_append,
            // …) accept `{ path: "..." }`; this heuristic is good
            // enough for v1.
            if let Some(p) = call
                .input
                .as_object()
                .and_then(|o| o.get("path"))
                .and_then(|v| v.as_str())
            {
                let path = p.to_string();
                if !files_recorded.contains(&path) {
                    files_recorded.push(path);
                }
            }
            turns.push(ChatTurn::ToolResult {
                tool_use_id: call.id.clone(),
                content,
                is_error,
            });
        }

        if round >= MAX_TOOL_ROUNDS {
            // Cap reached — surface a clear error rather than silently
            // truncate. The aggregated text so far still went out via
            // `on_chunk` so the user sees what the model produced.
            return Err(format!(
                "tool dispatch exceeded {MAX_TOOL_ROUNDS} rounds without a final answer"
            ));
        }
    }
}

/// Run one tool call through the registry and shape the executor's
/// outcome into the `(content, is_error)` pair the dispatch loop feeds
/// back to the model. Errors are surfaced verbatim so the model can
/// adjust on the next round.
async fn execute_tool_call(registry: &ToolRegistry, call: &ToolCall) -> (String, bool) {
    match registry.execute(&call.name, call.input.clone()).await {
        Ok(s) => (s, false),
        Err(ToolError::NotFound(name)) => (
            format!("tool '{name}' is not registered on this provider"),
            true,
        ),
        Err(ToolError::InvalidInput(msg)) => (format!("invalid input: {msg}"), true),
        Err(ToolError::ExecutionFailed(msg)) => (format!("execution failed: {msg}"), true),
    }
}

/// Translate the legacy `messages` payload (array of `{role, content}`
/// objects) into [`ChatTurn`]s. System messages are dropped here since
/// the provider receives the system prompt via the dedicated `system`
/// arg; assistant text becomes a tool-call-free assistant turn so the
/// model can see its own prior outputs.
fn messages_to_turns(messages: Vec<crate::provider::ChatMessage>) -> Vec<ChatTurn> {
    messages
        .into_iter()
        .filter_map(|m| match m.role {
            crate::provider::Role::User => Some(ChatTurn::User { content: m.content }),
            crate::provider::Role::Assistant => Some(ChatTurn::Assistant {
                content: m.content,
                tool_calls: Vec::new(),
            }),
            crate::provider::Role::System => None,
        })
        .collect()
}

// Same shape as handle_stream_chat — every error path records an
// `ActivityEntry` so the timeline reflects retrieval failures, embed
// failures, etc. alongside successes.
#[allow(clippy::too_many_lines, reason = "BL-037 records on every exit path; flow stays linear")]
async fn handle_stream_ask(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    activity: Option<ActivityRecorder>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let started_at = std::time::Instant::now();
    let messages: Vec<crate::provider::ChatMessage> = args
        .get("messages")
        .ok_or_else(|| exec_err("stream_ask: missing 'messages'"))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("stream_ask: messages decode: {e}")))
        })?;
    let session_id = args
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_string);
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(5);
    let question = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, crate::provider::Role::User))
        .map(|m| m.content.clone())
        .ok_or_else(|| exec_err("stream_ask: no user message in 'messages'"))?;

    // BL-037 — capture provider + model labels up front so error
    // entries (no AI cfg, retrieval failure, …) still record them
    // when present.
    let provider_label = ai_cfg.as_ref().map(|c| c.provider.clone());
    let model_label = ai_cfg.as_ref().and_then(|c| c.model.clone());

    let record_err = |err: String| {
        let rec = activity.clone();
        let session_id = session_id.clone();
        let provider_label = provider_label.clone();
        let model_label = model_label.clone();
        let prompt = question.clone();
        async move {
            record_activity_error(
                rec.as_ref(),
                &session_id,
                ActivitySurface::Ask,
                provider_label,
                model_label,
                prompt,
                started_at,
                &err,
            )
            .await;
        }
    };

    let Some(ai_cfg) = ai_cfg else {
        record_err("stream_ask: no AI chat provider configured".into()).await;
        return Err(exec_err("stream_ask: no AI chat provider configured"));
    };
    let Some(embed_cfg) = embed_cfg else {
        record_err("stream_ask: no embedding provider configured".into()).await;
        return Err(exec_err("stream_ask: no embedding provider configured"));
    };
    let ai = match build_ai_provider(&ai_cfg) {
        Ok(p) => p,
        Err(e) => {
            record_err(e.clone()).await;
            return Err(exec_err(e));
        }
    };
    let embedder = match build_embedding_provider(&embed_cfg) {
        Ok(p) => p,
        Err(e) => {
            record_err(e.clone()).await;
            return Err(exec_err(e));
        }
    };

    let sources = match crate::rag::retrieve(&ctx, embedder.as_ref(), &question, limit).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("stream_ask: retrieve: {e}");
            record_err(msg.clone()).await;
            return Err(exec_err(msg));
        }
    };
    let system = crate::rag::build_rag_prompt(&sources);

    let _ = ctx.publish(
        "com.nexus.ai.stream_start",
        serde_json::json!({
            "session_id": &session_id,
            "sources": &sources,
        }),
    );

    let ctx_chunk = Arc::clone(&ctx);
    let sid_chunk = session_id.clone();
    let chunk_idx = Arc::new(AtomicUsize::new(0));
    let on_chunk = {
        let chunk_idx = Arc::clone(&chunk_idx);
        move |chunk: String| {
            let idx = chunk_idx.fetch_add(1, Ordering::Relaxed);
            let _ = ctx_chunk.publish(
                "com.nexus.ai.stream_chunk",
                serde_json::json!({
                    "session_id": &sid_chunk,
                    "chunk": chunk,
                    "index": idx,
                }),
            );
        }
    };

    let text = match ai.chat_stream_with(&messages, Some(&system), &on_chunk).await {
        Ok(t) => t,
        Err(e) => {
            let msg = format!("stream_ask: {e}");
            record_err(msg.clone()).await;
            return Err(exec_err(msg));
        }
    };

    // BL-038: enrich sources with line ranges + 1-based numbering so the
    // shell can render `[N]` markers in the answer as clickable chips.
    let citations = crate::rag::build_citations(&ctx, &sources, &text).await;

    let _ = ctx.publish(
        "com.nexus.ai.stream_done",
        serde_json::json!({
            "session_id": &session_id,
            "text": &text,
            "sources": &sources,
            "citations": &citations,
        }),
    );

    if let Some(rec) = activity {
        // Files in the timeline = the RAG sources that grounded this
        // answer. Deduped while preserving retrieval order.
        let mut files: Vec<String> = Vec::new();
        for s in &sources {
            if !files.contains(&s.file_path) {
                files.push(s.file_path.clone());
            }
        }
        let entry = ActivityEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.clone(),
            surface: ActivitySurface::Ask,
            origin: "ai".into(),
            provider: provider_label,
            model: model_label,
            prompt: question.clone(),
            files,
            tool_calls: Vec::new(),
            outcome: ActivityOutcome::Ok,
            error: None,
            duration_ms: u64::try_from(started_at.elapsed().as_millis()).ok(),
        };
        rec.append(entry).await;
    }

    Ok(serde_json::json!({
        "session_id": session_id,
        "text": text,
        "sources": sources,
        "citations": citations,
    }))
}

// ─── BL-037 — activity timeline handlers ────────────────────────────────────

async fn handle_activity_list(
    activity: Option<ActivityRecorder>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiActivityListArgs = serde_json::from_value(args.clone()).unwrap_or_default();
    let Some(rec) = activity else {
        // Pre-`wire_context`: no recorder yet. Return an empty list
        // rather than erroring so the shell can poll on activate
        // without a race.
        return serde_json::to_value(&AiActivityListResult { entries: Vec::new() })
            .map_err(|e| exec_err(format!("activity_list: encode: {e}")));
    };
    // The on-disk log is oldest-first; the IPC contract returns
    // newest-first. Reverse + cap.
    let mut entries = rec.read_all().await?;
    entries.reverse();
    if let Some(limit) = parsed.limit {
        let limit_usize = limit as usize;
        entries.truncate(limit_usize);
    }
    serde_json::to_value(&AiActivityListResult { entries })
        .map_err(|e| exec_err(format!("activity_list: encode: {e}")))
}

async fn handle_activity_clear(
    activity: Option<ActivityRecorder>,
) -> Result<serde_json::Value, PluginError> {
    let Some(rec) = activity else {
        return Ok(serde_json::json!({ "cleared": false }));
    };
    rec.clear().await?;
    Ok(serde_json::json!({ "cleared": true }))
}

/// Relative path for the legacy single-session file. Kept for
/// backwards compatibility — callers that omit `id` on
/// `session_load` / `session_save` keep reading/writing this path.
const LEGACY_SESSION_RELPATH: &str = ".forge/chat_session.json";

/// Directory holding multi-session files. Each session lives at
/// `<SESSIONS_DIR>/<id>.json`.
const SESSIONS_DIR: &str = ".forge/chat/sessions";

/// Validate a caller-supplied session id. Keeps the filename safe
/// for disk + prevents path traversal.
fn validate_session_id(id: &str) -> Result<(), PluginError> {
    if id.is_empty() || id.len() > 64 {
        return Err(exec_err("session id must be 1..=64 chars".to_string()));
    }
    let ok = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(exec_err("session id must match [A-Za-z0-9_-]+".to_string()));
    }
    Ok(())
}

fn session_path(id: Option<&str>) -> Result<std::path::PathBuf, PluginError> {
    match id {
        None => Ok(std::path::PathBuf::from(LEGACY_SESSION_RELPATH)),
        Some(s) => {
            validate_session_id(s)?;
            Ok(std::path::PathBuf::from(SESSIONS_DIR).join(format!("{s}.json")))
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct SessionArgs {
    #[serde(default)]
    id: Option<String>,
}

async fn handle_session_load(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: SessionArgs = serde_json::from_value(args.clone()).unwrap_or_default();
    let path = session_path(parsed.id.as_deref())?;
    match ctx.read_file(&path).await {
        Ok(bytes) => {
            let parsed: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|e| exec_err(format!("session_load: invalid JSON on disk: {e}")))?;
            Ok(parsed)
        }
        // No session saved yet — return null rather than erroring so
        // fresh forges don't spam the UI with warnings.
        Err(_) => Ok(serde_json::Value::Null),
    }
}

async fn handle_session_save(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    // Caller may pass `{ id, ... }` or a bare session object. Pull
    // `id` out (if present) and persist the whole payload untouched.
    let id = args
        .as_object()
        .and_then(|o| o.get("id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    if let Some(ref s) = id {
        validate_session_id(s)?;
    }
    let path = session_path(id.as_deref())?;
    let encoded = serde_json::to_vec_pretty(args)
        .map_err(|e| exec_err(format!("session_save: encode: {e}")))?;
    ctx.write_file(&path, &encoded)
        .await
        .map_err(|e| exec_err(format!("session_save: write: {e}")))?;
    Ok(serde_json::json!({ "bytes": encoded.len(), "id": id }))
}

async fn handle_session_list(ctx: &KernelPluginContext) -> Result<serde_json::Value, PluginError> {
    let dir = std::path::Path::new(SESSIONS_DIR);
    let Ok(entries) = ctx.list_files(dir).await else {
        return Ok(serde_json::Value::Array(Vec::new()));
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    for path in entries {
        let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| validate_session_id(s).is_ok())
        else {
            continue;
        };
        let Ok(bytes) = ctx.read_file(&path).await else {
            continue;
        };
        let parsed: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let updated_at = parsed
            .get("updated_at")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        out.push(serde_json::json!({
            "id": stem,
            "title": title,
            "updated_at": updated_at,
            "bytes": bytes.len(),
        }));
    }
    Ok(serde_json::Value::Array(out))
}

async fn handle_session_delete(
    ctx: &KernelPluginContext,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    #[derive(serde::Deserialize)]
    struct Args {
        id: String,
    }
    let a: Args = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("session_delete: invalid args: {e}")))?;
    validate_session_id(&a.id)?;
    let path = session_path(Some(&a.id))?;
    match ctx.delete_file(&path).await {
        Ok(()) => Ok(serde_json::json!({ "deleted": true, "id": a.id })),
        Err(e) => Err(exec_err(format!("session_delete: {e}"))),
    }
}

// ─── Provider factories ─────────────────────────────────────────────────────

fn build_ai_provider(cfg: &AiConfig) -> Result<Box<dyn AiProvider>, String> {
    match cfg.provider.as_str() {
        "anthropic" => Ok(Box::new(AnthropicProvider::new(
            cfg.api_key.clone().unwrap_or_default(),
            cfg.model.clone(),
            cfg.max_tokens,
            cfg.tls_pinning_enabled,
        ))),
        "openai" => Ok(Box::new(OpenAiProvider::new(
            cfg.api_key.clone().unwrap_or_default(),
            cfg.model.clone(),
            cfg.max_tokens,
            cfg.tls_pinning_enabled,
        ))),
        "ollama" => Ok(Box::new(OllamaProvider::new(
            cfg.base_url.clone(),
            cfg.model.clone(),
            None,
        ))),
        other => Err(format!("unknown AI provider: {other}")),
    }
}

fn build_embedding_provider(cfg: &AiConfig) -> Result<Box<dyn EmbeddingProvider>, String> {
    match cfg.provider.as_str() {
        "openai" => Ok(Box::new(OpenAiProvider::new(
            cfg.api_key.clone().unwrap_or_default(),
            None,
            4096,
            cfg.tls_pinning_enabled,
        ))),
        "ollama" => Ok(Box::new(OllamaProvider::new(
            cfg.base_url.clone(),
            None,
            cfg.model.clone(),
        ))),
        "local" => build_local_embedding_provider(cfg),
        other => Err(format!("unknown embedding provider: {other}")),
    }
}

#[cfg(feature = "local-embeddings")]
fn build_local_embedding_provider(cfg: &AiConfig) -> Result<Box<dyn EmbeddingProvider>, String> {
    use crate::local_embedding::{LocalEmbedding, DEFAULT_LOCAL_MODEL};
    let model = cfg
        .local_embedding_model
        .as_deref()
        .unwrap_or(DEFAULT_LOCAL_MODEL);
    let backend = LocalEmbedding::new(model)
        .map_err(|e| format!("failed to initialize local embedding backend: {e}"))?;
    Ok(Box::new(backend))
}

#[cfg(not(feature = "local-embeddings"))]
fn build_local_embedding_provider(_cfg: &AiConfig) -> Result<Box<dyn EmbeddingProvider>, String> {
    Err("provider 'local' requires the 'local-embeddings' Cargo feature; \
         rebuild nexus-ai with --features local-embeddings to enable fastembed-rs"
        .to_string())
}

fn exec_err<S: Into<String>>(reason: S) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod aig05_local_embedding_config_tests {
    //! AIG-05 — set_config / config_snapshot / status round-trip for
    //! the `provider = "local"` embedding case. These cover the
    //! parse-layer wiring; the `local-embeddings` feature gate is
    //! exercised separately via `dimension_for` (only callable when
    //! the feature is on).

    use super::*;

    #[test]
    fn parse_config_field_lifts_local_model_into_canonical_slot() {
        let payload = serde_json::json!({
            "provider": "local",
            "model": "bge-small-en-v1.5",
        });
        let cfg = parse_config_field(&payload).unwrap().unwrap();
        assert_eq!(cfg.provider, "local");
        assert_eq!(cfg.model.as_deref(), Some("bge-small-en-v1.5"));
        assert_eq!(
            cfg.local_embedding_model.as_deref(),
            Some("bge-small-en-v1.5"),
            "local provider must populate the canonical slot",
        );
    }

    #[test]
    fn parse_config_field_does_not_lift_for_remote_providers() {
        let payload = serde_json::json!({
            "provider": "openai",
            "model": "text-embedding-3-small",
        });
        let cfg = parse_config_field(&payload).unwrap().unwrap();
        assert_eq!(cfg.local_embedding_model, None);
    }

    #[test]
    fn config_snapshot_exposes_local_embedding_model_only_for_local() {
        let local = AiConfig {
            provider: "local".into(),
            local_embedding_model: Some("bge-large-en-v1.5".into()),
            ..AiConfig::default()
        };
        let snap = config_snapshot(None, Some(&local));
        let model = snap
            .pointer("/embedding/local_embedding_model")
            .and_then(|v| v.as_str());
        assert_eq!(model, Some("bge-large-en-v1.5"));

        let remote = AiConfig {
            provider: "openai".into(),
            model: Some("text-embedding-3-small".into()),
            ..AiConfig::default()
        };
        let snap = config_snapshot(None, Some(&remote));
        // Remote providers omit the field rather than emitting null.
        assert!(snap.pointer("/embedding/local_embedding_model").is_none());
    }

    #[test]
    fn resolve_embedding_model_prefers_local_slot_for_local_provider() {
        let cfg = AiConfig {
            provider: "local".into(),
            model: Some("ignored".into()),
            local_embedding_model: Some("bge-base-en-v1.5".into()),
            ..AiConfig::default()
        };
        assert_eq!(
            resolve_embedding_model(&cfg).as_deref(),
            Some("bge-base-en-v1.5"),
        );
    }

    #[test]
    fn resolve_embedding_model_falls_through_to_chat_style_field_for_remote() {
        let cfg = AiConfig {
            provider: "openai".into(),
            model: Some("text-embedding-3-small".into()),
            ..AiConfig::default()
        };
        assert_eq!(
            resolve_embedding_model(&cfg).as_deref(),
            Some("text-embedding-3-small"),
        );
    }

    #[test]
    #[cfg(not(feature = "local-embeddings"))]
    fn resolve_embedding_dimension_returns_none_without_feature() {
        let cfg = AiConfig {
            provider: "local".into(),
            local_embedding_model: Some("bge-small-en-v1.5-int8".into()),
            ..AiConfig::default()
        };
        assert_eq!(resolve_embedding_dimension(&cfg), None);
    }

    #[test]
    #[cfg(feature = "local-embeddings")]
    fn resolve_embedding_dimension_resolves_known_local_model() {
        let cfg = AiConfig {
            provider: "local".into(),
            local_embedding_model: Some("bge-base-en-v1.5".into()),
            ..AiConfig::default()
        };
        // bge-base / nomic-embed → 768; see local_embedding::model_dimension.
        assert_eq!(resolve_embedding_dimension(&cfg), Some(768));
    }

    #[test]
    #[cfg(feature = "local-embeddings")]
    fn resolve_embedding_dimension_defaults_for_empty_identifier() {
        // Empty model string falls back to the canonical default
        // (BGE-small / 384-dim) per local_embedding::map_model.
        let cfg = AiConfig {
            provider: "local".into(),
            local_embedding_model: None,
            ..AiConfig::default()
        };
        assert_eq!(resolve_embedding_dimension(&cfg), Some(384));
    }

    #[test]
    fn resolve_embedding_dimension_returns_none_for_remote_providers() {
        let cfg = AiConfig {
            provider: "ollama".into(),
            model: Some("nomic-embed-text".into()),
            ..AiConfig::default()
        };
        assert_eq!(resolve_embedding_dimension(&cfg), None);
    }
}

#[cfg(test)]
mod read_only_filter_tests {
    use super::*;
    use crate::tools::{
        register_extended_builtins, register_storage_builtins, register_terminal_builtins,
    };
    use nexus_kernel::{
        Capability, CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore,
    };

    fn ctx_for_test() -> std::sync::Arc<KernelPluginContext> {
        let dir = tempfile::tempdir().unwrap();
        let kv: std::sync::Arc<dyn KvStore> = std::sync::Arc::new(InMemoryKvStore::new());
        let bus = std::sync::Arc::new(EventBus::new(16));
        let caps: CapabilitySet = [Capability::IpcCall].into_iter().collect();
        std::sync::Arc::new(
            KernelPluginContext::new("com.nexus.ai", "0.0.1", caps, kv, bus, dir.path(), None)
                .unwrap(),
        )
    }

    /// `AutoReadOnly` should keep `terminal_get_status` (read-only)
    /// and drop `terminal_run_saved` / `terminal_send_signal` (mutating).
    /// Pre-existing read-only entries (`read_file`, `search_forge`,
    /// `list_backlinks`, `git_log`) survive too; `write_file` does not.
    #[test]
    fn filter_keeps_read_only_terminal_tool_only() {
        let ctx = ctx_for_test();
        let mut full = ToolRegistry::new();
        register_storage_builtins(&mut full, std::sync::Arc::clone(&ctx));
        register_extended_builtins(&mut full, std::sync::Arc::clone(&ctx));
        register_terminal_builtins(&mut full, ctx);
        let filtered = filter_to_read_only(&full);
        let names: Vec<String> = filtered.schemas().into_iter().map(|s| s.name).collect();
        assert!(names.iter().any(|n| n == "terminal_get_status"));
        assert!(!names.iter().any(|n| n == "terminal_run_saved"));
        assert!(!names.iter().any(|n| n == "terminal_send_signal"));
        // Existing invariants — sanity-check we didn't break the
        // pre-BL-055 set while extending it.
        assert!(names.iter().any(|n| n == "read_file"));
        assert!(!names.iter().any(|n| n == "write_file"));
        assert!(names.iter().any(|n| n == "search_forge"));
        assert!(names.iter().any(|n| n == "list_backlinks"));
        assert!(names.iter().any(|n| n == "git_log"));
    }
}

#[cfg(test)]
mod system_floor_tests {
    use super::*;

    #[test]
    fn floor_alone_when_caller_is_none() {
        let out = compose_chat_system(None);
        assert_eq!(out, HOST_SYSTEM_PROMPT_FLOOR);
    }

    #[test]
    fn floor_alone_when_caller_is_empty() {
        assert_eq!(compose_chat_system(Some("")), HOST_SYSTEM_PROMPT_FLOOR);
        assert_eq!(compose_chat_system(Some("   \n")), HOST_SYSTEM_PROMPT_FLOOR);
    }

    #[test]
    fn floor_prepends_caller_supplied_system() {
        let out = compose_chat_system(Some("Be terse."));
        assert!(out.starts_with(HOST_SYSTEM_PROMPT_FLOOR));
        assert!(out.ends_with("Be terse."));
        // Separated by a blank line so neither blob bleeds into the other.
        assert!(out.contains("\n\nBe terse."));
    }

    #[test]
    fn floor_mentions_forge_relative_paths() {
        // Catch accidental floor-content drops: the path-confinement
        // line is the load-bearing safety nudge for tool-using chats.
        assert!(HOST_SYSTEM_PROMPT_FLOOR.contains("forge-relative"));
    }
}

#[cfg(test)]
mod session_id_tests {
    use super::*;

    #[test]
    fn accepts_alnum_dash_underscore() {
        assert!(validate_session_id("abc_123-xyz").is_ok());
        assert!(validate_session_id("A").is_ok());
    }

    #[test]
    fn rejects_empty_and_too_long() {
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id(&"a".repeat(65)).is_err());
        assert!(validate_session_id(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(validate_session_id("../etc").is_err());
        assert!(validate_session_id("a/b").is_err());
        assert!(validate_session_id("..").is_err());
        assert!(validate_session_id(".hidden").is_err());
    }

    #[test]
    fn rejects_whitespace_and_unicode() {
        assert!(validate_session_id("has space").is_err());
        assert!(validate_session_id("café").is_err());
    }

    #[test]
    fn session_path_routes_on_id_presence() {
        let legacy = session_path(None).unwrap();
        assert!(legacy.ends_with("chat_session.json"));
        let multi = session_path(Some("project-x")).unwrap();
        assert!(multi
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("chat/sessions/project-x.json"));
    }

    #[test]
    fn session_path_rejects_bad_id() {
        assert!(session_path(Some("../boom")).is_err());
    }
}

#[cfg(test)]
mod tool_dispatch_tests {
    use super::*;
    use crate::error::AiError;
    use crate::provider::{ChatMessage, ChatTurnOutput, Role};
    use crate::tools::{ToolExecutor, ToolSchema};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Stub provider that returns a scripted sequence of
    /// `ChatTurnOutput`s, one per `chat_turn_with_tools` call. Lets us
    /// drive the dispatch loop deterministically.
    struct ScriptedProvider {
        outputs: Mutex<Vec<ChatTurnOutput>>,
        turns_seen: Mutex<Vec<Vec<ChatTurn>>>,
    }

    impl ScriptedProvider {
        fn new(outputs: Vec<ChatTurnOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs),
                turns_seen: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl AiProvider for ScriptedProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _system: Option<&str>,
        ) -> Result<String, AiError> {
            // Reachable when callers exercise the `mode=complete` path
            // through the default `chat_stream_with` implementation,
            // which falls back to `chat`. Pop the next scripted output
            // and return its `text` (tool_calls are ignored — complete
            // mode forbids them anyway).
            let mut outs = self.outputs.lock().unwrap();
            if outs.is_empty() {
                return Err(AiError::Provider("script exhausted".to_string()));
            }
            let out = outs.remove(0);
            Ok(out.text)
        }

        async fn chat_turn_with_tools(
            &self,
            turns: &[ChatTurn],
            _system: Option<&str>,
            _tools: &[ToolSchema],
            on_chunk: &(dyn Fn(String) + Send + Sync),
        ) -> Result<ChatTurnOutput, AiError> {
            self.turns_seen.lock().unwrap().push(turns.to_vec());
            let mut outs = self.outputs.lock().unwrap();
            if outs.is_empty() {
                return Err(AiError::Provider("script exhausted".to_string()));
            }
            let out = outs.remove(0);
            if !out.text.is_empty() {
                on_chunk(out.text.clone());
            }
            Ok(out)
        }

        #[allow(clippy::unnecessary_literal_bound)]
        fn model_name(&self) -> &str {
            "scripted"
        }
    }

    /// Stub executor that records inputs and returns a fixed result.
    struct EchoExecutor {
        inputs: Mutex<Vec<serde_json::Value>>,
        result: String,
    }

    impl EchoExecutor {
        fn new(result: &str) -> Self {
            Self {
                inputs: Mutex::new(Vec::new()),
                result: result.to_string(),
            }
        }
    }

    #[async_trait]
    impl ToolExecutor for EchoExecutor {
        async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
            self.inputs.lock().unwrap().push(input);
            Ok(self.result.clone())
        }
    }

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: content.to_string(),
        }
    }

    #[tokio::test]
    async fn dispatch_loop_returns_text_when_no_tool_calls() {
        let provider = ScriptedProvider::new(vec![ChatTurnOutput {
            text: "hello".to_string(),
            tool_calls: Vec::new(),
        }]);
        let registry = ToolRegistry::new();
        let chunks = Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);

        let outcome = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("hi")],
            None,
            &on_chunk,
        )
        .await
        .expect("dispatch");
        assert_eq!(outcome.text, "hello");
        assert!(outcome.tool_calls.is_empty());
        assert!(outcome.files.is_empty());
        assert_eq!(chunks.lock().unwrap().as_slice(), &["hello"]);
    }

    #[tokio::test]
    async fn dispatch_loop_executes_tool_and_loops() {
        // Round 1: model asks for `read_file`. Round 2: model wraps up.
        let provider = ScriptedProvider::new(vec![
            ChatTurnOutput {
                text: "let me check".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tc_1".to_string(),
                    name: "echo".to_string(),
                    input: serde_json::json!({"x": 1}),
                }],
            },
            ChatTurnOutput {
                text: "all done".to_string(),
                tool_calls: Vec::new(),
            },
        ]);
        let mut registry = ToolRegistry::new();
        let exec = std::sync::Arc::new(EchoExecutor::new("FILE_BODY"));
        registry.register(
            "echo",
            ToolSchema {
                name: "echo".to_string(),
                description: "echo".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            std::sync::Arc::clone(&exec) as std::sync::Arc<dyn ToolExecutor>,
        );

        let chunks = Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);

        let outcome = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("read it")],
            None,
            &on_chunk,
        )
        .await
        .expect("dispatch");

        // Aggregated text spans both rounds.
        assert!(outcome.text.contains("let me check"));
        assert!(outcome.text.contains("all done"));
        // BL-037 — the tool call is recorded with ok=true and contributes no
        // file path (echo doesn't carry a `path` input field).
        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.tool_calls[0].name, "echo");
        assert!(outcome.tool_calls[0].ok);
        assert!(outcome.files.is_empty());

        // Tool was invoked once with the model's args.
        let inputs = exec.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0], serde_json::json!({"x": 1}));

        // Second provider call saw the assistant + tool_result turns.
        let seen = provider.turns_seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let round_two = &seen[1];
        // initial user + assistant tool-call + tool-result
        assert_eq!(round_two.len(), 3);
        assert!(matches!(round_two[1], ChatTurn::Assistant { .. }));
        match &round_two[2] {
            ChatTurn::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tc_1");
                assert_eq!(content, "FILE_BODY");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[tokio::test]
    async fn dispatch_loop_marks_unknown_tool_as_error() {
        let provider = ScriptedProvider::new(vec![
            ChatTurnOutput {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "tc_x".to_string(),
                    name: "missing_tool".to_string(),
                    input: serde_json::json!({}),
                }],
            },
            ChatTurnOutput {
                text: "recovered".to_string(),
                tool_calls: Vec::new(),
            },
        ]);
        let registry = ToolRegistry::new();
        let on_chunk = |_: String| {};
        let outcome = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("call something")],
            None,
            &on_chunk,
        )
        .await
        .expect("dispatch");
        assert!(outcome.text.contains("recovered"));
        // BL-037 — unknown-tool entry is recorded as ok=false.
        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.tool_calls[0].name, "missing_tool");
        assert!(!outcome.tool_calls[0].ok);

        let seen = provider.turns_seen.lock().unwrap();
        let round_two = &seen[1];
        match &round_two[round_two.len() - 1] {
            ChatTurn::ToolResult {
                content, is_error, ..
            } => {
                assert!(*is_error);
                assert!(content.contains("missing_tool"));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[tokio::test]
    async fn dispatch_loop_caps_at_max_rounds() {
        // Provider keeps asking for the same tool every round.
        let mut script = Vec::new();
        for _ in 0..(MAX_TOOL_ROUNDS + 2) {
            script.push(ChatTurnOutput {
                text: "loop".to_string(),
                tool_calls: vec![ToolCall {
                    id: "tc".to_string(),
                    name: "echo".to_string(),
                    input: serde_json::json!({}),
                }],
            });
        }
        let provider = ScriptedProvider::new(script);
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo",
            ToolSchema {
                name: "echo".to_string(),
                description: "echo".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            std::sync::Arc::new(EchoExecutor::new("ok")) as std::sync::Arc<dyn ToolExecutor>,
        );

        let on_chunk = |_: String| {};
        let err = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("loop")],
            None,
            &on_chunk,
        )
        .await
        .expect_err("must hit cap");
        assert!(err.contains(&MAX_TOOL_ROUNDS.to_string()));
    }

    #[tokio::test]
    async fn messages_to_turns_drops_system_keeps_user_assistant() {
        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "ignore me".to_string(),
            },
            ChatMessage {
                role: Role::User,
                content: "hi".to_string(),
            },
            ChatMessage {
                role: Role::Assistant,
                content: "hello".to_string(),
            },
        ];
        let turns = messages_to_turns(messages);
        assert_eq!(turns.len(), 2);
        assert!(matches!(turns[0], ChatTurn::User { .. }));
        match &turns[1] {
            ChatTurn::Assistant {
                content,
                tool_calls,
            } => {
                assert_eq!(content, "hello");
                assert!(tool_calls.is_empty());
            }
            _ => panic!("expected Assistant"),
        }
    }

    // ─── BL-010 / BL-011 / BL-034 — `mode=complete` engine path ─────────

    fn chat_args_complete() -> AiStreamChatArgs {
        AiStreamChatArgs {
            messages: Vec::new(),
            system: None,
            session_id: None,
            mode: Some(AiStreamChatMode::Complete),
            tools: None,
            max_tokens: None,
            stop: None,
            trim: None,
            surface: None,
        }
    }

    /// `mode=complete` MUST physically bypass the tool-dispatch loop.
    /// We use a registry that contains `echo` and a script whose first
    /// (and only consumed) output carries `tool_calls`. If the engine
    /// were running the tool loop those calls would either execute or
    /// surface as tool-result turns; in complete mode the calls are
    /// silently dropped because `chat_stream_with` (default impl ->
    /// `chat`) returns text only.
    #[tokio::test]
    async fn complete_mode_skips_tool_loop() {
        let provider = ScriptedProvider::new(vec![
            ChatTurnOutput {
                text: "the answer is 42".to_string(),
                // Even with a tool_call scripted, complete-mode must
                // ignore it: the dispatch loop is the only path that
                // would observe it, and we are intentionally
                // bypassing it.
                tool_calls: vec![ToolCall {
                    id: "tc_should_not_run".to_string(),
                    name: "echo".to_string(),
                    input: serde_json::json!({"x": 1}),
                }],
            },
            // A second scripted output exists to prove the engine
            // never came back for round 2.
            ChatTurnOutput {
                text: "MUST NOT BE EMITTED".to_string(),
                tool_calls: Vec::new(),
            },
        ]);
        // Registry has `echo`, but complete mode advertises no tools
        // and never enters the dispatch loop.
        let mut registry = ToolRegistry::new();
        let exec = std::sync::Arc::new(EchoExecutor::new("SIDE_EFFECT"));
        registry.register(
            "echo",
            ToolSchema {
                name: "echo".to_string(),
                description: "echo".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            std::sync::Arc::clone(&exec) as std::sync::Arc<dyn ToolExecutor>,
        );

        let chunks = Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);

        let args = chat_args_complete();
        let messages = vec![user_msg("what's the answer?")];
        let text = run_complete(&provider, &messages, None, &args, &on_chunk)
            .await
            .expect("complete");

        // Only the first scripted output's text reached us — the
        // second (which tests for accidental looping) was untouched.
        assert_eq!(text, "the answer is 42");

        // Tool was never executed: complete mode does not run the
        // dispatch loop, and the registry is irrelevant to this path.
        assert!(exec.inputs.lock().unwrap().is_empty());

        // `chat_turn_with_tools` (the loop's entry point) was never
        // called — only the `chat`/`chat_stream_with` path consumed
        // a scripted output.
        assert!(provider.turns_seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn complete_mode_strips_prompt_echo() {
        // Model echoes the trailing chunk of the prompt before its
        // continuation — a common instruction-tuned-model behaviour.
        let provider = ScriptedProvider::new(vec![ChatTurnOutput {
            text: "The quick brown fox jumps over the lazy dog.".to_string(),
            tool_calls: Vec::new(),
        }]);
        let on_chunk = |_: String| {};
        let mut args = chat_args_complete();
        args.trim = Some(true);
        let messages = vec![user_msg("Continue: The quick brown")];
        let text = run_complete(&provider, &messages, None, &args, &on_chunk)
            .await
            .expect("complete");
        // Prompt-echo (the "The quick brown" prefix) is gone.
        assert!(
            !text.starts_with("The quick brown"),
            "echoed prefix should be stripped, got: {text:?}"
        );
        // The continuation is preserved.
        assert!(text.contains("fox jumps over the lazy dog"));
    }

    #[tokio::test]
    async fn complete_mode_trims_to_natural_break() {
        // Two complete sentences plus a dangling fragment — the
        // fragment after the last `.` should be clipped.
        let provider = ScriptedProvider::new(vec![ChatTurnOutput {
            text: "First sentence. Second sentence. And then a half".to_string(),
            tool_calls: Vec::new(),
        }]);
        let on_chunk = |_: String| {};
        let mut args = chat_args_complete();
        args.trim = Some(true);
        // Different prompt → no echo to strip; we're isolating the
        // natural-break behaviour.
        let messages = vec![user_msg("write something")];
        let text = run_complete(&provider, &messages, None, &args, &on_chunk)
            .await
            .expect("complete");
        assert_eq!(text, "First sentence. Second sentence.");
    }

    #[test]
    fn apply_stop_truncates() {
        // Multiple stops — the earliest hit wins.
        let stops = vec!["END".to_string(), "\n\n".to_string()];
        let cut = apply_stop("hello\n\nworld END more", &stops);
        assert_eq!(cut, "hello");

        // No stop matches → return the input unchanged.
        let untouched = apply_stop("nothing to cut here", &stops);
        assert_eq!(untouched, "nothing to cut here");

        // Empty stops list is a no-op.
        let empty: Vec<String> = Vec::new();
        assert_eq!(apply_stop("keep all", &empty), "keep all");

        // Empty-string entries are ignored (would otherwise match at
        // position 0 and truncate everything).
        let with_empty = vec![String::new(), "STOP".to_string()];
        assert_eq!(apply_stop("hello STOP world", &with_empty), "hello ");
    }

    /// Regression: with `mode` absent, the chat path is unchanged —
    /// the dispatch loop runs and aggregates assistant text exactly
    /// like the pre-change baseline. This is the same shape as
    /// `dispatch_loop_returns_text_when_no_tool_calls` but exercised
    /// via a path identical to what `handle_stream_chat` builds when
    /// `mode` is `None`.
    #[tokio::test]
    async fn chat_mode_unchanged() {
        let provider = ScriptedProvider::new(vec![ChatTurnOutput {
            text: "hello".to_string(),
            tool_calls: Vec::new(),
        }]);
        let registry = ToolRegistry::new();
        let chunks = Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);

        let outcome = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("hi")],
            None,
            &on_chunk,
        )
        .await
        .expect("dispatch");
        assert_eq!(outcome.text, "hello");
        assert_eq!(chunks.lock().unwrap().as_slice(), &["hello"]);
        // The dispatch-loop path was the one that ran (turns_seen
        // populated), proving the chat path uses tool dispatch.
        assert_eq!(provider.turns_seen.lock().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod semantic_search_dispatch_tests {
    //! BL-040 — exercise the `HANDLER_SEMANTIC_SEARCH` arm of
    //! [`AiCorePlugin::dispatch_async`] without making any network
    //! calls. The handler validates `query` and `embed_cfg` before it
    //! tries to embed, so we can drive both code paths cheaply by
    //! arranging for one of those checks to fire.
    use super::*;
    use nexus_kernel::{
        CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore,
    };
    use std::sync::Arc;

    fn wired_plugin() -> AiCorePlugin {
        wired_plugin_with_caps(CapabilitySet::default())
    }

    /// Wire a plugin against a temp forge with `caps` granted. The
    /// activity-timeline tests need `FsRead` + `FsWrite` so the
    /// recorder's read-modify-write cycle on `.forge/ai-activity.log`
    /// can actually persist; the original semantic-search tests run
    /// fine with the default (empty) set.
    ///
    /// The `.forge/` subdirectory is pre-created so writes to
    /// `.forge/<file>` succeed without an extra `mkdir -p`. Mirrors
    /// what production does at storage `on_init`.
    fn wired_plugin_with_caps(caps: CapabilitySet) -> AiCorePlugin {
        let mut plugin = AiCorePlugin::new();
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();
        std::fs::create_dir_all(dir_path.join(".forge")).unwrap();
        std::mem::forget(dir);
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let bus = Arc::new(EventBus::new(16));
        let ctx = KernelPluginContext::new(
            "com.nexus.ai",
            "0.0.1",
            caps,
            kv,
            bus,
            &dir_path,
            None,
        )
        .unwrap();
        plugin.wire_context(Arc::new(ctx));
        plugin
    }

    fn fs_caps() -> CapabilitySet {
        use nexus_kernel::Capability;
        CapabilitySet::from_iter([Capability::FsRead, Capability::FsWrite])
    }

    #[tokio::test]
    async fn semantic_search_handler_routes_through_dispatch_async() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_SEMANTIC_SEARCH, &serde_json::json!({}))
            .expect("HANDLER_SEMANTIC_SEARCH must be async");
        let err = fut.await.expect_err("missing query should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("missing 'query'"),
            "expected query-missing error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn semantic_search_handler_requires_embed_provider() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(
                HANDLER_SEMANTIC_SEARCH,
                &serde_json::json!({ "query": "hello" }),
            )
            .expect("HANDLER_SEMANTIC_SEARCH must be async");
        let err = fut.await.expect_err("no embed cfg should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("no AI embedding provider configured"),
            "expected embed-cfg error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn index_status_handler_returns_default_snapshot_when_daemon_unstarted() {
        // No `wire_context` here — the daemon has never spawned, so
        // `running` is `false` and counters are all zero. The handler
        // must still return a well-formed JSON object so the shell
        // badge can render "idle".
        let mut plugin = AiCorePlugin::new();
        let fut = plugin
            .dispatch_async(HANDLER_INDEX_STATUS, &serde_json::json!({}))
            .expect("HANDLER_INDEX_STATUS must be async");
        let value = fut.await.expect("snapshot must succeed");
        assert_eq!(value.get("running"), Some(&serde_json::json!(false)));
        assert_eq!(value.get("indexed_files"), Some(&serde_json::json!(0)));
        assert_eq!(value.get("pending_files"), Some(&serde_json::json!(0)));
        assert_eq!(value.get("total_seen"), Some(&serde_json::json!(0)));
        assert_eq!(value.get("last_error"), Some(&serde_json::json!(null)));
    }

    #[test]
    fn index_status_handler_id_is_fourteen() {
        // Pin the wire id for bootstrap manifest / shell drift detection.
        assert_eq!(HANDLER_INDEX_STATUS, 14);
    }

    #[test]
    fn semantic_search_handler_id_is_thirteen() {
        // Pin the wire id so external callers (bootstrap manifest,
        // shell plugin, MCP) don't drift silently.
        assert_eq!(HANDLER_SEMANTIC_SEARCH, 13);
    }

    #[test]
    fn enrich_handler_ids_are_pinned() {
        // BL-045: bootstrap manifest + shell plugin both reference
        // these constants — bumping them is a wire break.
        assert_eq!(HANDLER_ENRICH_FILE, 15);
        assert_eq!(HANDLER_ENRICH_APPLY, 16);
    }

    #[tokio::test]
    async fn enrich_file_requires_path() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_ENRICH_FILE, &serde_json::json!({}))
            .expect("HANDLER_ENRICH_FILE must be async");
        let err = fut.await.expect_err("missing path should error");
        assert!(format!("{err}").contains("missing 'path'"));
    }

    #[tokio::test]
    async fn enrich_file_requires_ai_provider() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(
                HANDLER_ENRICH_FILE,
                &serde_json::json!({ "path": "note.md" }),
            )
            .expect("HANDLER_ENRICH_FILE must be async");
        let err = fut.await.expect_err("no AI cfg should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("no AI chat provider configured"),
            "expected ai-cfg error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn enrich_apply_requires_proposal() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_ENRICH_APPLY, &serde_json::json!({}))
            .expect("HANDLER_ENRICH_APPLY must be async");
        let err = fut.await.expect_err("missing proposal should error");
        assert!(format!("{err}").contains("missing 'proposal'"));
    }

    // ─── BL-037 — activity timeline IPC handler tests ────────────────────

    #[test]
    fn activity_handler_ids_are_pinned() {
        // Bootstrap manifest + shell plugin both reference these
        // constants — bumping them is a wire break.
        assert_eq!(HANDLER_ACTIVITY_LIST, 18);
        assert_eq!(HANDLER_ACTIVITY_CLEAR, 19);
    }

    #[tokio::test]
    async fn activity_list_returns_empty_when_recorder_unwired() {
        // No `wire_context` so `self.activity` is `None`. Handler
        // contract: return an empty list rather than erroring so the
        // shell pane hydrates cleanly even before bootstrap finishes.
        let mut plugin = AiCorePlugin::new();
        let fut = plugin
            .dispatch_async(HANDLER_ACTIVITY_LIST, &serde_json::json!({}))
            .expect("HANDLER_ACTIVITY_LIST must be async");
        let value = fut.await.expect("activity_list must succeed");
        assert_eq!(value, serde_json::json!({ "entries": [] }));
    }

    #[tokio::test]
    async fn activity_clear_returns_cleared_false_when_recorder_unwired() {
        let mut plugin = AiCorePlugin::new();
        let fut = plugin
            .dispatch_async(HANDLER_ACTIVITY_CLEAR, &serde_json::json!({}))
            .expect("HANDLER_ACTIVITY_CLEAR must be async");
        let value = fut.await.expect("activity_clear must succeed");
        assert_eq!(value, serde_json::json!({ "cleared": false }));
    }

    #[tokio::test]
    async fn activity_recorder_round_trips_through_disk() {
        // End-to-end: build a wired plugin (so the recorder is bound
        // to a real `KernelPluginContext` over a temp forge), append
        // two entries, list them back. The list IPC contract is
        // newest-first.
        let plugin = wired_plugin_with_caps(fs_caps());
        let recorder = plugin.activity.clone().expect("recorder wired");

        let mut entry1 = ActivityEntry::now_ai(
            "sess-1".into(),
            nexus_types::activity::ActivitySurface::Chat,
        );
        entry1.prompt = "first prompt".into();
        recorder.append(entry1.clone()).await;

        let mut entry2 = ActivityEntry::now_ai(
            "sess-2".into(),
            nexus_types::activity::ActivitySurface::Ask,
        );
        entry2.prompt = "second prompt".into();
        entry2.files = vec!["notes/a.md".into(), "notes/b.md".into()];
        recorder.append(entry2.clone()).await;

        // Read via the IPC handler — exercises both the recorder's
        // `read_all` and the handler's reverse-to-newest-first.
        let mut plugin = plugin;
        let fut = plugin
            .dispatch_async(HANDLER_ACTIVITY_LIST, &serde_json::json!({}))
            .expect("HANDLER_ACTIVITY_LIST must be async");
        let value = fut.await.expect("activity_list");
        let entries = value
            .get("entries")
            .and_then(|v| v.as_array())
            .expect("entries array");
        assert_eq!(entries.len(), 2, "two entries expected");
        // Newest first: entry2's prompt should be at index 0.
        assert_eq!(entries[0].get("prompt").unwrap(), "second prompt");
        assert_eq!(entries[1].get("prompt").unwrap(), "first prompt");
        // Files survived the JSONL round-trip.
        let files = entries[0]
            .get("files")
            .and_then(|v| v.as_array())
            .expect("files array");
        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    async fn activity_list_respects_limit_arg() {
        let plugin = wired_plugin_with_caps(fs_caps());
        let recorder = plugin.activity.clone().expect("recorder wired");
        for i in 0..3 {
            let mut e = ActivityEntry::now_ai(
                format!("sess-{i}"),
                nexus_types::activity::ActivitySurface::Chat,
            );
            e.prompt = format!("prompt {i}");
            recorder.append(e).await;
        }

        let mut plugin = plugin;
        let fut = plugin
            .dispatch_async(HANDLER_ACTIVITY_LIST, &serde_json::json!({ "limit": 2 }))
            .expect("HANDLER_ACTIVITY_LIST must be async");
        let value = fut.await.expect("activity_list");
        let entries = value
            .get("entries")
            .and_then(|v| v.as_array())
            .expect("entries array");
        assert_eq!(entries.len(), 2);
        // Newest two first.
        assert_eq!(entries[0].get("prompt").unwrap(), "prompt 2");
        assert_eq!(entries[1].get("prompt").unwrap(), "prompt 1");
    }

    #[tokio::test]
    async fn activity_clear_truncates_log() {
        let plugin = wired_plugin_with_caps(fs_caps());
        let recorder = plugin.activity.clone().expect("recorder wired");
        let mut e = ActivityEntry::now_ai(
            "s".into(),
            nexus_types::activity::ActivitySurface::Chat,
        );
        e.prompt = "to be wiped".into();
        recorder.append(e).await;

        let mut plugin = plugin;
        // Sanity: there's one entry.
        let fut = plugin
            .dispatch_async(HANDLER_ACTIVITY_LIST, &serde_json::json!({}))
            .unwrap();
        let value = fut.await.unwrap();
        assert_eq!(
            value
                .get("entries")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(1)
        );

        // Clear, then list again — must be empty.
        let fut = plugin
            .dispatch_async(HANDLER_ACTIVITY_CLEAR, &serde_json::json!({}))
            .unwrap();
        let value = fut.await.unwrap();
        assert_eq!(value, serde_json::json!({ "cleared": true }));

        let fut = plugin
            .dispatch_async(HANDLER_ACTIVITY_LIST, &serde_json::json!({}))
            .unwrap();
        let value = fut.await.unwrap();
        assert_eq!(value, serde_json::json!({ "entries": [] }));
    }
}

#[cfg(test)]
mod bl102_tls_pinning_status_tests {
    //! BL-102 follow-up — `tls_pinning_effective` mirrors the
    //! `build_client` gate so the `nexus ai status` `tls_pinned` field
    //! tracks the live HTTP-client configuration.

    use super::{tls_pinning_effective, AiConfig};

    #[test]
    fn config_flag_enables_pinning_regardless_of_env() {
        let mut cfg = AiConfig::default();
        cfg.tls_pinning_enabled = true;
        // The OR with the env var means a `true` config flag short-
        // circuits to `true`; this assertion holds whether or not the
        // ambient `NEXUS_TLS_PINNING` is set.
        assert!(tls_pinning_effective(Some(&cfg)));
    }

    #[test]
    fn no_config_and_no_flag_means_pinning_off_unless_env_set() {
        // Match build_client semantics: in the absence of the env
        // opt-in, an unconfigured AI surface reports unpinned.
        let env_opt_in = std::env::var("NEXUS_TLS_PINNING")
            .map(|v| v == "1")
            .unwrap_or(false);
        assert_eq!(tls_pinning_effective(None), env_opt_in);

        let cfg = AiConfig::default();
        assert_eq!(tls_pinning_effective(Some(&cfg)), env_opt_in);
    }
}
