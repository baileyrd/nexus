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
use std::sync::Arc;

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::Serialize;

use crate::anthropic::AnthropicProvider;
use crate::config::{detect_embedding_provider, detect_provider, AiConfig};
use crate::embedding::EmbeddingProvider;
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;
use crate::provider::AiProvider;
use crate::{rag, vectorstore};

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

/// Core plugin for AI integration.
pub struct AiCorePlugin {
    ai_config: Option<AiConfig>,
    embed_config: Option<AiConfig>,
    /// Plugin-facing kernel context, installed via [`CorePlugin::wire_context`]
    /// after the shared plugin loader + dispatcher are assembled. Handlers
    /// clone the `Arc` into their spawned futures. `None` if a handler fires
    /// before bootstrap finishes wiring — those handlers return a clear error.
    context: Option<Arc<KernelPluginContext>>,
}

impl AiCorePlugin {
    /// Construct an unstarted plugin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ai_config: None,
            embed_config: None,
            context: None,
        }
    }

    /// Return the detected AI chat-provider configuration, if any.
    #[must_use]
    pub fn config(&self) -> Option<&AiConfig> {
        self.ai_config.as_ref()
    }
}

impl Default for AiCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePlugin for AiCorePlugin {
    /// Detect AI and embedding providers from environment variables.
    fn on_init(&mut self) -> Result<(), PluginError> {
        self.ai_config = detect_provider();
        self.embed_config = detect_embedding_provider();
        if let Some(cfg) = &self.ai_config {
            tracing::debug!(plugin_id = PLUGIN_ID, provider = %cfg.provider, "AI provider detected");
        } else {
            tracing::debug!(plugin_id = PLUGIN_ID, "no AI provider detected; AI features disabled");
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
            return Ok(config_snapshot(self.ai_config.as_ref(), self.embed_config.as_ref()));
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
            let response = config_snapshot(self.ai_config.as_ref(), self.embed_config.as_ref());
            return Some(Box::pin(async move { Ok(response) }));
        }

        let ctx = self.context.clone();
        let ai_cfg = self.ai_config.clone();
        let embed_cfg = self.embed_config.clone();
        let args = args.clone();

        Some(Box::pin(async move {
            let ctx = ctx.ok_or_else(|| {
                exec_err("AI plugin context not wired (bootstrap incomplete)")
            })?;
            match handler_id {
                HANDLER_ASK => handle_ask(&ctx, ai_cfg, embed_cfg, &args).await,
                HANDLER_INDEX_FILE => handle_index_file(&ctx, embed_cfg, &args).await,
                HANDLER_VECTORSTORE_COUNT => handle_vectorstore_count(&ctx).await,
                HANDLER_STATUS => handle_status(&ctx, ai_cfg, embed_cfg).await,
                HANDLER_STREAM_CHAT => handle_stream_chat(ctx, ai_cfg, &args).await,
                HANDLER_STREAM_ASK => {
                    handle_stream_ask(ctx, ai_cfg, embed_cfg, &args).await
                }
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
    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
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

/// Build a detected-provider snapshot (synchronous — no I/O).
fn config_snapshot(
    ai_cfg: Option<&AiConfig>,
    embed_cfg: Option<&AiConfig>,
) -> serde_json::Value {
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
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

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

    let text = ai
        .chat_stream_with(&messages, system.as_deref(), &on_chunk)
        .await
        .map_err(|e| exec_err(format!("stream_chat: {e}")))?;

    let _ = ctx.publish(
        "com.nexus.ai.stream_done",
        serde_json::json!({"session_id": &session_id, "text": &text}),
    );

    Ok(serde_json::json!({"session_id": session_id, "text": text}))
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
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
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
    let embed_cfg = embed_cfg
        .ok_or_else(|| exec_err("stream_ask: no embedding provider configured"))?;
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
        return Err(exec_err(
            "session id must be 1..=64 chars".to_string(),
        ));
    }
    let ok = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(exec_err(
            "session id must match [A-Za-z0-9_-]+".to_string(),
        ));
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
            let parsed: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
                exec_err(format!("session_load: invalid JSON on disk: {e}"))
            })?;
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

async fn handle_session_list(
    ctx: &KernelPluginContext,
) -> Result<serde_json::Value, PluginError> {
    let dir = std::path::Path::new(SESSIONS_DIR);
    let entries = match ctx.list_files(dir).await {
        Ok(v) => v,
        Err(_) => return Ok(serde_json::Value::Array(Vec::new())),
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
        let bytes = match ctx.read_file(&path).await {
            Ok(b) => b,
            Err(_) => continue,
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
        "ollama" => Ok(Box::new(OllamaProvider::new(
            cfg.base_url.clone(),
            None,
        ))),
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
