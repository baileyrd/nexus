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

use std::sync::{Arc, RwLock};

use nexus_kernel::KernelPluginContext;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};

use crate::activity_log::ActivityRecorder;
use crate::config::{detect_embedding_provider, detect_provider, AiConfig};
use crate::generate_docs::handle_generate_docs;
use crate::indexing_daemon::{self, DaemonMsg, EmbedderFactory, IndexingDaemon, SharedStatus};
use crate::tools::{
    register_extended_builtins, register_storage_builtins, register_terminal_builtins, ToolRegistry,
};
use tokio::sync::mpsc::UnboundedSender;

// Re-export shared helpers so the in-file test modules (which use
// `super::*`) keep seeing the same symbols they did pre-split.
use crate::handlers::activity::{handle_activity_clear, handle_activity_list};
use crate::handlers::ask::{handle_ask, handle_generate};
use crate::handlers::config::handle_set_config;
use crate::handlers::enrich::{handle_enrich_apply, handle_enrich_file};
use crate::handlers::entity::{handle_enrich_entity, handle_infer_entity_relations};
use crate::handlers::index::{
    handle_index_file, handle_index_trigger, handle_status, handle_vectorstore_count,
};
use crate::handlers::predict::handle_predict;
use crate::handlers::propose::handle_propose_tool_calls;
use crate::handlers::search::{handle_embed_text, handle_entity_recall, handle_semantic_search};
use crate::handlers::session::{
    handle_session_delete, handle_session_list, handle_session_load, handle_session_save,
};
use crate::handlers::stream_ask::handle_stream_ask;
use crate::handlers::stream_chat::handle_stream_chat;

// Re-export shared helpers and module-private items used by the
// in-file `#[cfg(test)]` modules below. These previously lived in
// this file — keep them in-scope (via `use`) so the test modules'
// `use super::*;` continues to resolve.
#[cfg(test)]
use crate::handlers::config::parse_config_field;
#[cfg(test)]
use crate::handlers::session::{session_path, validate_session_id};
#[cfg(test)]
use crate::handlers::shared::{
    apply_stop, compose_chat_system, filter_to_read_only, messages_to_turns,
    resolve_embedding_dimension, resolve_embedding_model, run_complete, run_tool_dispatch_loop,
    tls_pinning_effective, HOST_SYSTEM_PROMPT_FLOOR, MAX_TOOL_ROUNDS,
};
use crate::handlers::shared::{
    build_embedding_provider, config_snapshot, resolve_credentials_payload,
};

// Re-exports for callers outside `core_plugin` that historically
// reached for `crate::core_plugin::{...}` private items
// (`generate_docs.rs` does this for `build_ai_provider`).
pub(crate) use crate::handlers::shared::{build_ai_provider, exec_err};

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
/// (newest-first). Args [`crate::ipc::AiActivityListArgs`] (optionally caps the
/// number returned); result [`crate::ipc::AiActivityListResult`].
pub const HANDLER_ACTIVITY_LIST: u32 = 18;

/// BL-037 — `activity_clear`: truncate the activity log to zero
/// bytes. No args, returns `{ cleared: true }`.
pub const HANDLER_ACTIVITY_CLEAR: u32 = 19;

/// G7 — `propose_tool_calls`: single-turn provider call that returns
/// the model's tool-use blocks WITHOUT executing them. Used by
/// `nexus-agent` (ADR 0023) to derive a `Plan` for later
/// approval-gated execution. Args [`crate::ipc::AiProposeArgs`], reply
/// [`crate::ipc::AiProposeReply`].
pub const HANDLER_PROPOSE_TOOL_CALLS: u32 = 20;

/// BL-116 — `generate_docs`. Args: [`crate::ipc::AiGenerateDocsArgs`];
/// reply [`crate::ipc::AiGenerateDocsReply`]. Resolves a symbol from
/// the BL-114 `code_symbols` index, reads its source range,
/// assembles a 1-hop context (parent + sibling symbols, since
/// BL-114's index has no call-edges), prompts the configured AI
/// provider for a language-appropriate docblock, and returns the
/// formatted output. The caller is responsible for splicing the
/// docblock above `insert_line` — write-back is not performed here.
pub const HANDLER_GENERATE_DOCS: u32 = 22;

/// BL-117 — return the live AI chat provider's resolved credentials
/// (provider name, base URL, api_key) so sibling subsystems like
/// `nexus-audio` can talk to the same provider endpoint without
/// asking the user to configure a second key. Reads from the
/// `ai_config` `RwLock` so a runtime `set_config` push by the shell
/// is honoured. Reply shape:
/// ```json
/// { "provider": "openai", "api_key": "...", "base_url": "https://..." | null,
///   "model":   "..." | null }
/// ```
/// or `null` when no AI provider is configured.
///
/// The API key in the reply is sensitive — gate calls under the
/// existing `ipc.call` capability the caller already needs to reach
/// here, and audit each call via the activity log.
pub const HANDLER_RESOLVE_CREDENTIALS: u32 = 21;

/// BL-128 close — `entity_recall`: FAISS-backed entity recall.
/// Embeds the query through the configured provider, queries the
/// shared chunk vectorstore, filters hits to files under
/// `entities/`, groups by file (max score per entity), resolves
/// stems back to full entity records via `com.nexus.storage::entity_get`,
/// and returns `EntityRecallHitRow`s ranked by descending score.
///
/// Callers fall back to the substring-ranking `com.nexus.storage::entity_search`
/// when no embedder is configured. Args: [`crate::ipc::EntityRecallArgs`].
/// Returns [`crate::ipc::EntityRecallResult`].
pub const HANDLER_ENTITY_RECALL: u32 = 23;

/// BL-129 close — `enrich_entity`: read one entity through storage,
/// optionally gather supporting snippets via RAG, ask the AI provider
/// for a richer description, and (unless `dry_run`) write the result
/// back via `entity_upsert`. Args: [`crate::ipc::EnrichEntityArgs`].
/// Returns [`crate::ipc::EnrichEntityResult`].
pub const HANDLER_ENRICH_ENTITY: u32 = 24;

/// BL-129 close — `infer_entity_relations`: look up the entity,
/// gather similar entities via `entity_recall`, prompt the AI for
/// proposed new relations, filter to targets that actually exist and
/// aren't already related, and (unless `dry_run`) append them at
/// `confidence: 0.5` via `entity_upsert`. Args:
/// [`crate::ipc::InferEntityRelationsArgs`]. Returns
/// [`crate::ipc::InferEntityRelationsResult`].
pub const HANDLER_INFER_ENTITY_RELATIONS: u32 = 25;

/// BL-139 — `predict`: per-keystroke fill-in-middle code completion.
/// Args: [`crate::ipc::AiPredictArgs`]. Returns
/// [`crate::ipc::AiPredictReply`]. Routes to Ollama's `/api/generate`
/// (with `suffix`) when the configured provider is `ollama`, falls
/// back to a chat-shaped FIM prompt for OpenAI / Anthropic.
pub const HANDLER_PREDICT: u32 = 26;

/// `embed_text` — embed one or more strings with the configured embedding
/// provider and return the dense vectors. Lets other plugins (e.g. the memory
/// engine) reuse the one embedding path rather than carrying their own model.
pub const HANDLER_EMBED_TEXT: u32 = 27;

/// `generate` — plain prompt → text completion via the chat provider, no RAG.
/// The synthesis primitive behind memory's LLM-wiki pages.
pub const HANDLER_GENERATE: u32 = 28;

/// C26 (#379) — `cancel_stream`. Args: `{ "session_id": String }`;
/// Returns: `{ "cancelled": bool }` (`false` = no live stream by that
/// id). Fires the cooperative cancel flag the streaming providers
/// check between SSE chunks.
pub const HANDLER_CANCEL_STREAM: u32 = 29;

/// Plugin ids this plugin requires already loaded. `ai-runtime` is
/// hard — `wire_context` grabs the shared tokio worker-pool handle
/// the runtime publishes. `storage` underwrites every RAG / vector
/// call. `security` provides credential + TLS pin types.
pub const MANIFEST_DEPS: &[&str] = &[
    "com.nexus.security",
    "com.nexus.storage",
    "com.nexus.ai.runtime",
];

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::ai::register`. Order
/// matches the pre-SD-06 bootstrap registration so the emitted
/// manifest is byte-identical. `HANDLER_PREDICT` is intentionally not
/// listed — the predict surface is reached via the streaming endpoints.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("ask", HANDLER_ASK),
    ("index_file", HANDLER_INDEX_FILE),
    ("vectorstore_count", HANDLER_VECTORSTORE_COUNT),
    ("status", HANDLER_STATUS),
    ("config", HANDLER_CONFIG),
    ("stream_chat", HANDLER_STREAM_CHAT),
    ("cancel_stream", HANDLER_CANCEL_STREAM),
    ("stream_ask", HANDLER_STREAM_ASK),
    ("session_load", HANDLER_SESSION_LOAD),
    ("session_save", HANDLER_SESSION_SAVE),
    ("session_list", HANDLER_SESSION_LIST),
    ("session_delete", HANDLER_SESSION_DELETE),
    ("set_config", HANDLER_SET_CONFIG),
    ("semantic_search", HANDLER_SEMANTIC_SEARCH),
    ("index_status", HANDLER_INDEX_STATUS),
    ("enrich_file", HANDLER_ENRICH_FILE),
    ("enrich_apply", HANDLER_ENRICH_APPLY),
    ("index_trigger", HANDLER_INDEX_TRIGGER),
    ("activity_list", HANDLER_ACTIVITY_LIST),
    ("activity_clear", HANDLER_ACTIVITY_CLEAR),
    ("propose_tool_calls", HANDLER_PROPOSE_TOOL_CALLS),
    ("resolve_credentials", HANDLER_RESOLVE_CREDENTIALS),
    ("generate_docs", HANDLER_GENERATE_DOCS),
    ("entity_recall", HANDLER_ENTITY_RECALL),
    ("enrich_entity", HANDLER_ENRICH_ENTITY),
    ("infer_entity_relations", HANDLER_INFER_ENTITY_RELATIONS),
    ("embed_text", HANDLER_EMBED_TEXT),
    ("generate", HANDLER_GENERATE),
];

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
        if handler_id == HANDLER_RESOLVE_CREDENTIALS {
            let ai = self.ai_config.read().ok().and_then(|g| g.clone());
            return Ok(resolve_credentials_payload(ai.as_ref()));
        }
        if handler_id == HANDLER_INDEX_STATUS {
            let snap = indexing_daemon::snapshot(&self.index_status);
            return serde_json::to_value(&snap)
                .map_err(|e| exec_err(format!("index_status: serialize: {e}")));
        }
        Err(PluginError::HandlerIsAsyncOnly { handler_id })
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

        // BL-117 — sync resolve_credentials. Mirrors the
        // HANDLER_CONFIG fall-through so a caller using either
        // dispatch path observes identical behaviour.
        if handler_id == HANDLER_RESOLVE_CREDENTIALS {
            let ai = self.ai_config.read().ok().and_then(|g| g.clone());
            let response = resolve_credentials_payload(ai.as_ref());
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
                    exec_err("AI plugin context not wired (bootstrap incomplete)".to_string())
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
            return Some(Box::pin(
                async move { handle_activity_clear(activity).await },
            ));
        }

        // BL-139 — `predict` doesn't need the kernel context (no tool
        // loop, no recorder, no event publish) so we route it ahead
        // of the ctx-required block. Lets ghost-text predictions fire
        // even if the editor mounts during early-bootstrap.
        if handler_id == HANDLER_PREDICT {
            let ai_cfg = self.ai_config.read().ok().and_then(|g| g.clone());
            let args = args.clone();
            return Some(Box::pin(async move { handle_predict(ai_cfg, &args).await }));
        }

        let ctx = self.context.clone();
        let tools = self.tools.clone();
        let activity = self.activity.clone();
        let ai_cfg = self.ai_config.read().ok().and_then(|g| g.clone());
        let embed_cfg = self.embed_config.read().ok().and_then(|g| g.clone());
        let args = args.clone();

        Some(Box::pin(async move {
            let ctx = ctx.ok_or_else(|| {
                exec_err("AI plugin context not wired (bootstrap incomplete)".to_string())
            })?;
            match handler_id {
                HANDLER_ASK => handle_ask(&ctx, ai_cfg, embed_cfg, &args).await,
                HANDLER_INDEX_FILE => handle_index_file(&ctx, embed_cfg, &args).await,
                HANDLER_VECTORSTORE_COUNT => handle_vectorstore_count(&ctx).await,
                HANDLER_STATUS => handle_status(&ctx, ai_cfg, embed_cfg).await,
                HANDLER_STREAM_CHAT => {
                    handle_stream_chat(ctx, ai_cfg, tools, activity, &args).await
                }
                HANDLER_CANCEL_STREAM => {
                    let session_id = args
                        .get("session_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            crate::handlers::shared::exec_err(
                                "cancel_stream: missing 'session_id' string".to_string(),
                            )
                        })?;
                    let cancelled = crate::cancel::cancel(session_id);
                    Ok(serde_json::json!({ "cancelled": cancelled }))
                }
                HANDLER_STREAM_ASK => {
                    handle_stream_ask(ctx, ai_cfg, embed_cfg, activity, &args).await
                }
                HANDLER_SESSION_LOAD => handle_session_load(&ctx, &args).await,
                HANDLER_SESSION_SAVE => handle_session_save(&ctx, &args).await,
                HANDLER_SESSION_LIST => handle_session_list(&ctx).await,
                HANDLER_SESSION_DELETE => handle_session_delete(&ctx, &args).await,
                HANDLER_SEMANTIC_SEARCH => handle_semantic_search(&ctx, embed_cfg, &args).await,
                HANDLER_EMBED_TEXT => handle_embed_text(embed_cfg, &args).await,
                HANDLER_GENERATE => handle_generate(ai_cfg, &args).await,
                HANDLER_ENRICH_FILE => handle_enrich_file(&ctx, ai_cfg, embed_cfg, &args).await,
                HANDLER_ENRICH_APPLY => handle_enrich_apply(&ctx, &args).await,
                HANDLER_PROPOSE_TOOL_CALLS => {
                    handle_propose_tool_calls(ctx, ai_cfg, tools, &args).await
                }
                HANDLER_GENERATE_DOCS => handle_generate_docs(ctx, ai_cfg, &args).await,
                HANDLER_ENTITY_RECALL => handle_entity_recall(&ctx, embed_cfg, &args).await,
                HANDLER_ENRICH_ENTITY => handle_enrich_entity(&ctx, ai_cfg, embed_cfg, &args).await,
                HANDLER_INFER_ENTITY_RELATIONS => {
                    handle_infer_entity_relations(&ctx, ai_cfg, embed_cfg, &args).await
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

        // P2-06 — pull the debounce window from the current AiConfig
        // (per-forge) instead of the static DEFAULT_DEBOUNCE so an
        // `[ai] indexing_debounce_secs = N` override actually takes
        // effect. `ai_config` is the `set_config`-mutated handle the
        // daemon also consults for its embedder factory.
        let debounce = self
            .ai_config
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(crate::AiConfig::indexing_debounce))
            .unwrap_or(crate::indexing_daemon::DEFAULT_DEBOUNCE);
        match IndexingDaemon::start_with_debounce(
            Arc::clone(&ctx),
            Arc::clone(&self.index_status),
            factory,
            debounce,
        ) {
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
    use crate::ipc::{AiStreamChatArgs, AiStreamChatMode};
    use crate::provider::{ChatMessage, ChatTurn, ChatTurnOutput, Role, ToolCall};
    use crate::tools::{ToolError, ToolExecutor, ToolSchema};
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
    impl crate::provider::AiProvider for ScriptedProvider {
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

        let outcome =
            run_tool_dispatch_loop(&provider, &registry, vec![user_msg("hi")], None, &on_chunk)
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

    /// Phase 5.5 (2c) — the rich `turns` wire form round-trips into
    /// provider-native [`ChatTurn`]s with assistant `tool_use` ↔
    /// `tool_result` linkage preserved, unlike the lossy text-only
    /// `messages_to_turns` path.
    #[test]
    fn ai_turns_preserve_tool_use_linkage() {
        use crate::handlers::shared::ai_turns_to_chat_turns;
        use crate::ipc::{AiChatTurn, AiTurnToolCall};

        let wire = vec![
            AiChatTurn::User {
                content: "read notes.md".to_string(),
            },
            AiChatTurn::Assistant {
                content: "on it".to_string(),
                tool_calls: vec![AiTurnToolCall {
                    id: "toolu_1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({ "path": "notes.md" }),
                }],
            },
            AiChatTurn::ToolResult {
                tool_use_id: "toolu_1".to_string(),
                content: "file contents".to_string(),
                is_error: false,
            },
        ];
        let turns = ai_turns_to_chat_turns(&wire);
        assert_eq!(turns.len(), 3);
        match &turns[1] {
            ChatTurn::Assistant {
                content,
                tool_calls,
            } => {
                assert_eq!(content, "on it");
                assert_eq!(tool_calls.len(), 1, "tool call must survive the boundary");
                assert_eq!(tool_calls[0].id, "toolu_1");
                assert_eq!(tool_calls[0].name, "read_file");
            }
            other => panic!("expected Assistant with a tool call, got {other:?}"),
        }
        match &turns[2] {
            ChatTurn::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_1");
                assert_eq!(content, "file contents");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    /// The internally-tagged `AiChatTurn` wire form survives a JSON
    /// round-trip (the agent serializes it, the AI handler decodes it).
    #[test]
    fn ai_chat_turn_json_round_trip() {
        use crate::ipc::{AiChatTurn, AiTurnToolCall};

        let turns = vec![
            AiChatTurn::User {
                content: "hi".to_string(),
            },
            AiChatTurn::Assistant {
                content: String::new(),
                tool_calls: vec![AiTurnToolCall {
                    id: "t1".to_string(),
                    name: "grep".to_string(),
                    input: serde_json::json!({ "q": "x" }),
                }],
            },
            AiChatTurn::ToolResult {
                tool_use_id: "t1".to_string(),
                content: "boom".to_string(),
                is_error: true,
            },
        ];
        let json = serde_json::to_string(&turns).expect("serialize");
        // Internally tagged on `kind`.
        assert!(json.contains("\"kind\":\"user\""));
        assert!(json.contains("\"kind\":\"assistant\""));
        assert!(json.contains("\"kind\":\"tool_result\""));
        let back: Vec<AiChatTurn> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, turns);
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
                tool_calls: vec![ToolCall {
                    id: "tc_should_not_run".to_string(),
                    name: "echo".to_string(),
                    input: serde_json::json!({"x": 1}),
                }],
            },
            ChatTurnOutput {
                text: "MUST NOT BE EMITTED".to_string(),
                tool_calls: Vec::new(),
            },
        ]);
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

        let outcome =
            run_tool_dispatch_loop(&provider, &registry, vec![user_msg("hi")], None, &on_chunk)
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
    use nexus_kernel::{CapabilitySet, EventBus, InMemoryKvStore, KernelPluginContext, KvStore};
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
        let ctx = KernelPluginContext::new("com.nexus.ai", "0.0.1", caps, kv, bus, &dir_path, None)
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
    async fn embed_text_handler_requires_embed_provider() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_EMBED_TEXT, &serde_json::json!({ "text": "hello" }))
            .expect("HANDLER_EMBED_TEXT must be async");
        let err = fut.await.expect_err("no embed cfg should error");
        assert!(
            format!("{err}").contains("no AI embedding provider configured"),
            "expected embed-cfg error, got: {err}"
        );
    }

    #[tokio::test]
    async fn embed_text_handler_rejects_missing_input() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_EMBED_TEXT, &serde_json::json!({}))
            .expect("HANDLER_EMBED_TEXT must be async");
        let err = fut.await.expect_err("missing texts/text should error");
        assert!(
            format!("{err}").contains("missing 'texts'"),
            "expected missing-input error, got: {err}"
        );
    }

    #[tokio::test]
    async fn embed_text_handler_empty_texts_returns_empty_without_provider() {
        // Empty input short-circuits before the provider is needed.
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_EMBED_TEXT, &serde_json::json!({ "texts": [] }))
            .expect("HANDLER_EMBED_TEXT must be async");
        let value = fut.await.expect("empty input must succeed");
        assert_eq!(value.get("embeddings"), Some(&serde_json::json!([])));
        assert_eq!(value.get("dimension"), Some(&serde_json::json!(0)));
    }

    #[test]
    fn embed_text_handler_id_is_twentyseven() {
        assert_eq!(HANDLER_EMBED_TEXT, 27);
    }

    #[tokio::test]
    async fn generate_handler_requires_chat_provider() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_GENERATE, &serde_json::json!({ "prompt": "hi" }))
            .expect("HANDLER_GENERATE must be async");
        let err = fut.await.expect_err("no chat cfg should error");
        assert!(
            format!("{err}").contains("no AI chat provider configured"),
            "expected chat-cfg error, got: {err}"
        );
    }

    #[tokio::test]
    async fn generate_handler_rejects_missing_prompt() {
        let mut plugin = wired_plugin();
        let fut = plugin
            .dispatch_async(HANDLER_GENERATE, &serde_json::json!({}))
            .expect("HANDLER_GENERATE must be async");
        let err = fut.await.expect_err("missing prompt should error");
        assert!(
            format!("{err}").contains("missing 'prompt'"),
            "expected missing-prompt error, got: {err}"
        );
    }

    #[test]
    fn generate_handler_id_is_twentyeight() {
        assert_eq!(HANDLER_GENERATE, 28);
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
        use nexus_types::activity::ActivityEntry;
        let plugin = wired_plugin_with_caps(fs_caps());
        let recorder = plugin.activity.clone().expect("recorder wired");

        let mut entry1 = ActivityEntry::now_ai(
            "sess-1".into(),
            nexus_types::activity::ActivitySurface::Chat,
        );
        entry1.prompt = "first prompt".into();
        recorder.append(entry1.clone()).await;

        let mut entry2 =
            ActivityEntry::now_ai("sess-2".into(), nexus_types::activity::ActivitySurface::Ask);
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
        use nexus_types::activity::ActivityEntry;
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
        use nexus_types::activity::ActivityEntry;
        let plugin = wired_plugin_with_caps(fs_caps());
        let recorder = plugin.activity.clone().expect("recorder wired");
        let mut e = ActivityEntry::now_ai("s".into(), nexus_types::activity::ActivitySurface::Chat);
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
        let cfg = AiConfig {
            tls_pinning_enabled: true,
            ..Default::default()
        };
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
