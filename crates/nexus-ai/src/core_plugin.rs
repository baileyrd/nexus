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
