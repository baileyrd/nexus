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

use crate::anthropic::AnthropicProvider;
use crate::config::{detect_embedding_provider, detect_provider, AiConfig};
use crate::embedding::EmbeddingProvider;
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;
use crate::provider::{AiProvider, ChatTurn, ToolCall};
use crate::tools::{register_storage_builtins, ToolError, ToolRegistry};
use crate::{rag, vectorstore};

/// Hard cap on tool-call rounds inside a single `stream_chat`
/// invocation. Each round = one provider call + N tool executions.
/// 8 is enough for realistic agent flows (read a file, search,
/// summarise, write) without letting a runaway loop pin the kernel.
const MAX_TOOL_ROUNDS: usize = 8;

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
///   { ai?:        { provider, model?, api_key?, base_url? } | null,
///     embedding?: { provider, model?, api_key?, base_url? } | null }
///
/// A `null` clears that side; an absent key leaves it untouched.
pub const HANDLER_SET_CONFIG: u32 = 12;

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

        // set_config: in-memory only, no I/O — but we model it as async
        // for symmetry with the rest of the surface and so the shell
        // can `await` confirmation that the new credentials are live
        // before emitting a "configured" UI event.
        if handler_id == HANDLER_SET_CONFIG {
            let ai_handle = Arc::clone(&self.ai_config);
            let embed_handle = Arc::clone(&self.embed_config);
            let args = args.clone();
            return Some(Box::pin(async move {
                handle_set_config(ai_handle, embed_handle, &args)
            }));
        }

        let ctx = self.context.clone();
        let tools = self.tools.clone();
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
                HANDLER_STREAM_CHAT => handle_stream_chat(ctx, ai_cfg, tools, &args).await,
                HANDLER_STREAM_ASK => handle_stream_ask(ctx, ai_cfg, embed_cfg, &args).await,
                HANDLER_SESSION_LOAD => handle_session_load(&ctx, &args).await,
                HANDLER_SESSION_SAVE => handle_session_save(&ctx, &args).await,
                HANDLER_SESSION_LIST => handle_session_list(&ctx).await,
                HANDLER_SESSION_DELETE => handle_session_delete(&ctx, &args).await,
                _ => Err(exec_err(format!("unknown handler id {handler_id}"))),
            }
        }))
    }

    /// Capture the kernel plugin context so async handlers can issue nested
    /// `ipc_call`s into storage through the canonical plugin surface.
    /// Also seeds the tool registry with the storage-backed built-ins
    /// (`read_file`, `write_file`) so the streaming dispatch loop has a
    /// non-empty toolbox without each frontend opting in.
    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        let mut registry = ToolRegistry::new();
        register_storage_builtins(&mut registry, Arc::clone(&ctx));
        self.tools = Some(Arc::new(registry));
        self.context = Some(ctx);
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
    Ok(serde_json::json!({
        "ai_provider": ai_cfg.as_ref().map(|c| c.provider.clone()),
        "ai_model": ai_cfg.as_ref().and_then(|c| c.model.clone()),
        "embedding_provider": embed_cfg.as_ref().map(|c| c.provider.clone()),
        "indexed_chunks": count,
    }))
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
    ai_handle: Arc<RwLock<Option<AiConfig>>>,
    embed_handle: Arc<RwLock<Option<AiConfig>>>,
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
    Ok(Some(AiConfig {
        provider,
        model,
        api_key,
        base_url,
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
    }
    fn view(cfg: &AiConfig) -> ConfigView<'_> {
        ConfigView {
            provider: cfg.provider.as_str(),
            model: cfg.model.as_deref(),
            base_url: cfg.base_url.as_deref(),
            has_api_key: cfg.api_key.is_some(),
        }
    }
    serde_json::json!({
        "ai": ai_cfg.map(view),
        "embedding": embed_cfg.map(view),
    })
}

async fn handle_stream_chat(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    tools: Option<Arc<ToolRegistry>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let messages: Vec<crate::provider::ChatMessage> = args
        .get("messages")
        .ok_or_else(|| exec_err("stream_chat: missing 'messages'"))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("stream_chat: messages decode: {e}")))
        })?;
    let system = args
        .get("system")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let session_id = args
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_string);

    let ai_cfg = ai_cfg.ok_or_else(|| exec_err("stream_chat: no AI chat provider configured"))?;
    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;

    let _ = ctx.publish(
        "com.nexus.ai.stream_start",
        serde_json::json!({"session_id": &session_id}),
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

    let registry = tools.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
    let text = run_tool_dispatch_loop(
        ai.as_ref(),
        registry.as_ref(),
        messages,
        system.as_deref(),
        &on_chunk,
    )
    .await
    .map_err(|e| exec_err(format!("stream_chat: {e}")))?;

    let _ = ctx.publish(
        "com.nexus.ai.stream_done",
        serde_json::json!({"session_id": &session_id, "text": &text}),
    );

    Ok(serde_json::json!({"session_id": session_id, "text": text}))
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
async fn run_tool_dispatch_loop(
    ai: &dyn AiProvider,
    registry: &ToolRegistry,
    messages: Vec<crate::provider::ChatMessage>,
    system: Option<&str>,
    on_chunk: &(dyn Fn(String) + Send + Sync),
) -> Result<String, String> {
    let mut turns = messages_to_turns(messages);
    let schemas = registry.schemas();

    let mut aggregated = String::new();
    let mut round: usize = 0;

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
            return Ok(aggregated);
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

async fn handle_stream_ask(
    ctx: Arc<KernelPluginContext>,
    ai_cfg: Option<AiConfig>,
    embed_cfg: Option<AiConfig>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
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

    let ai_cfg = ai_cfg.ok_or_else(|| exec_err("stream_ask: no AI chat provider configured"))?;
    let embed_cfg =
        embed_cfg.ok_or_else(|| exec_err("stream_ask: no embedding provider configured"))?;
    let ai = build_ai_provider(&ai_cfg).map_err(exec_err)?;
    let embedder = build_embedding_provider(&embed_cfg).map_err(exec_err)?;

    let sources = crate::rag::retrieve(&ctx, embedder.as_ref(), &question, limit)
        .await
        .map_err(|e| exec_err(format!("stream_ask: retrieve: {e}")))?;
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

    let text = ai
        .chat_stream_with(&messages, Some(&system), &on_chunk)
        .await
        .map_err(|e| exec_err(format!("stream_ask: {e}")))?;

    let _ = ctx.publish(
        "com.nexus.ai.stream_done",
        serde_json::json!({
            "session_id": &session_id,
            "text": &text,
            "sources": &sources,
        }),
    );

    Ok(serde_json::json!({
        "session_id": session_id,
        "text": text,
        "sources": sources,
    }))
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
        ))),
        "openai" => Ok(Box::new(OpenAiProvider::new(
            cfg.api_key.clone().unwrap_or_default(),
            cfg.model.clone(),
            cfg.max_tokens,
        ))),
        "ollama" => Ok(Box::new(OllamaProvider::new(
            cfg.base_url.clone(),
            cfg.model.clone(),
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
        ))),
        "ollama" => Ok(Box::new(OllamaProvider::new(cfg.base_url.clone(), None))),
        other => Err(format!("unknown embedding provider: {other}")),
    }
}

fn exec_err<S: Into<String>>(reason: S) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: reason.into(),
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
            unimplemented!("ScriptedProvider only implements chat_turn_with_tools")
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

        let text = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("hi")],
            None,
            &on_chunk,
        )
        .await
        .expect("dispatch");
        assert_eq!(text, "hello");
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

        let text = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("read it")],
            None,
            &on_chunk,
        )
        .await
        .expect("dispatch");

        // Aggregated text spans both rounds.
        assert!(text.contains("let me check"));
        assert!(text.contains("all done"));

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
        let text = run_tool_dispatch_loop(
            &provider,
            &registry,
            vec![user_msg("call something")],
            None,
            &on_chunk,
        )
        .await
        .expect("dispatch");
        assert!(text.contains("recovered"));

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
}
