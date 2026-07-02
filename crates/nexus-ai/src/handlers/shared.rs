//! Shared infrastructure used across the AI plugin's IPC handlers.
//!
//! Lives under `handlers/` (rather than `core_plugin.rs`) so each
//! per-handler module can pull in just the helpers it needs. Nothing
//! here is part of the plugin's public API — every item is
//! `pub(crate)` for use by the dispatcher (`core_plugin.rs`) and the
//! sibling handler modules.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use nexus_kernel::{Events as _, KernelPluginContext};
use serde::Serialize;

use crate::activity_log::ActivityRecorder;
use crate::anthropic::AnthropicProvider;
use crate::config::AiConfig;
use crate::embedding::EmbeddingProvider;
use crate::ipc::{AiStreamAskMessage, AiStreamAskRole, AiStreamChatArgs, AiStreamChatMode};
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;
use crate::provider::{AiProvider, ChatMessage, ChatTurn, ToolCall};
use crate::tools::{ToolError, ToolRegistry};
use nexus_types::activity::{ActivityEntry, ActivityOutcome, ActivitySurface};

/// Reverse-DNS identifier for this plugin.
pub(crate) const PLUGIN_ID: &str = "com.nexus.ai";

/// Hard cap on tool-call rounds inside a single `stream_chat`
/// invocation. Each round = one provider call + N tool executions.
/// 8 is enough for realistic agent flows (read a file, search,
/// summarise, write) without letting a runaway loop pin the kernel.
pub(crate) const MAX_TOOL_ROUNDS: usize = 8;

/// Host-owned system-prompt floor (G3) applied to every
/// `mode=chat` call. Prepended to whatever `system` the caller
/// supplied — never replaces it. Skipped for `mode=complete` (the
/// ghost-completion contract is "raw completion text, no host
/// scaffolding"). Kept terse: every chat call carries this on the
/// wire, so token cost matters.
pub(crate) const HOST_SYSTEM_PROMPT_FLOOR: &str =
    "You are operating inside Nexus, the user's personal knowledge forge — \
     a directory of plain-text files (mostly Markdown). All file paths you \
     see or emit are forge-relative; never use absolute paths or paths \
     containing `..`. Prefer using available tools (reading, searching, \
     writing) over guessing. When you modify a file, make minimal targeted \
     edits and preserve the user's existing structure and tone.";

/// Names of read-only built-in tools shipped via
/// [`crate::tools::register_storage_builtins`] / `register_extended_builtins`
/// / `register_terminal_builtins`. Used by [`filter_to_read_only`]
/// to honour [`crate::ipc::AiToolPolicy::AutoReadOnly`] (ADR 0022 Phase 2).
pub(crate) const READ_ONLY_TOOL_NAMES: &[&str] = &[
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
pub(crate) fn filter_to_read_only(source: &ToolRegistry) -> ToolRegistry {
    let mut filtered = ToolRegistry::new();
    for schema in source.schemas() {
        if !READ_ONLY_TOOL_NAMES.contains(&schema.name.as_str()) {
            continue;
        }
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
pub(crate) struct ForwardingExecutor {
    pub(crate) target_name: String,
    pub(crate) source: std::sync::Arc<ToolRegistry>,
}

#[async_trait::async_trait]
impl crate::tools::ToolExecutor for ForwardingExecutor {
    async fn execute(&self, input: serde_json::Value) -> Result<String, crate::tools::ToolError> {
        self.source.execute(&self.target_name, input).await
    }
}

/// Compose the effective system prompt for `mode=chat`. Returns the
/// floor when `caller` is empty/`None`; returns `floor + "\n\n" +
/// caller` otherwise.
pub(crate) fn compose_chat_system(caller: Option<&str>) -> String {
    match caller.map(str::trim).filter(|s| !s.is_empty()) {
        Some(c) => format!("{HOST_SYSTEM_PROMPT_FLOOR}\n\n{c}"),
        None => HOST_SYSTEM_PROMPT_FLOOR.to_string(),
    }
}

nexus_plugins::define_dispatch_helpers!(pub(crate));

// ─── Provider factories ─────────────────────────────────────────────────────

/// V14 (`repo-review-2026-06-10.md`) — latch so the unpinned-remote
/// warning fires once per process, not on every provider rebuild.
static TLS_PINNING_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// One-shot operator warning when a remote provider will send API
/// credentials over a connection without TLS certificate pinning.
/// Pinning stays opt-in (BL-102 default-off shipping posture) because
/// it requires seeded per-host pins — but the trade-off should be a
/// conscious choice, not default silence.
fn warn_if_unpinned_remote_provider(cfg: &AiConfig) {
    let remote = matches!(cfg.provider.as_str(), "anthropic" | "openai");
    let has_key = cfg.api_key.as_deref().is_some_and(|k| !k.is_empty());
    if remote
        && has_key
        && !tls_pinning_effective(Some(cfg))
        && !TLS_PINNING_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        tracing::warn!(
            audit = true,
            provider = %cfg.provider,
            "AI provider credentials will be sent without TLS certificate \
             pinning (standard WebPKI validation only). Pinning is opt-in \
             (BL-102): seed pins and set tls_pinning_enabled = true in \
             ai.toml, or export NEXUS_TLS_PINNING=1.",
        );
    }
}

pub(crate) fn build_ai_provider(cfg: &AiConfig) -> Result<Box<dyn AiProvider>, String> {
    warn_if_unpinned_remote_provider(cfg);
    // P2-04: per-provider default model. cfg.model is the per-request
    // override; the per-provider field is the forge-level default
    // (ai.toml). Either falls through to the provider's built-in
    // constant when both are None.
    match cfg.provider.as_str() {
        "anthropic" => Ok(Box::new(AnthropicProvider::new(
            cfg.api_key.clone().unwrap_or_default(),
            cfg.model.clone().or_else(|| cfg.anthropic_model.clone()),
            cfg.max_tokens,
            cfg.tls_pinning_enabled,
        ))),
        "openai" => Ok(Box::new(OpenAiProvider::new(
            cfg.api_key.clone().unwrap_or_default(),
            cfg.model.clone().or_else(|| cfg.openai_chat_model.clone()),
            cfg.max_tokens,
            cfg.tls_pinning_enabled,
        ))),
        "ollama" => Ok(Box::new(OllamaProvider::with_fim_temperature(
            cfg.base_url.clone(),
            cfg.model.clone().or_else(|| cfg.ollama_chat_model.clone()),
            None,
            cfg.ollama_temperature,
        ))),
        other => Err(format!("unknown AI provider: {other}")),
    }
}

pub(crate) fn build_embedding_provider(
    cfg: &AiConfig,
) -> Result<Box<dyn EmbeddingProvider>, String> {
    match cfg.provider.as_str() {
        "openai" => Ok(Box::new(OpenAiProvider::new(
            cfg.api_key.clone().unwrap_or_default(),
            cfg.openai_embedding_model.clone(),
            4096,
            cfg.tls_pinning_enabled,
        ))),
        "ollama" => Ok(Box::new(OllamaProvider::with_fim_temperature(
            cfg.base_url.clone(),
            None,
            cfg.model
                .clone()
                .or_else(|| cfg.ollama_embedding_model.clone()),
            cfg.ollama_temperature,
        ))),
        "local" => build_local_embedding_provider(cfg),
        other => Err(format!("unknown embedding provider: {other}")),
    }
}

#[cfg(feature = "local-embeddings")]
pub(crate) fn build_local_embedding_provider(
    cfg: &AiConfig,
) -> Result<Box<dyn EmbeddingProvider>, String> {
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
pub(crate) fn build_local_embedding_provider(
    _cfg: &AiConfig,
) -> Result<Box<dyn EmbeddingProvider>, String> {
    Err(
        "provider 'local' requires the 'local-embeddings' Cargo feature; \
         rebuild nexus-ai with --features local-embeddings to enable fastembed-rs"
            .to_string(),
    )
}

// ─── Status / config helpers (sync, no I/O) ─────────────────────────────────

/// BL-102 follow-up — mirrors the gate in
/// [`crate::http_client::build_client`] so the wire field tracks the
/// HTTP client that actually got built. Pinning is on iff the AI
/// config flag is set OR `NEXUS_TLS_PINNING=1` is in the environment.
pub(crate) fn tls_pinning_effective(ai_cfg: Option<&AiConfig>) -> bool {
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
pub(crate) fn resolve_embedding_model(cfg: &AiConfig) -> Option<String> {
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
pub(crate) fn resolve_embedding_dimension(cfg: &AiConfig) -> Option<usize> {
    if cfg.provider != "local" {
        return None;
    }
    let id = cfg.local_embedding_model.as_deref().unwrap_or("");
    crate::local_embedding::dimension_for(id)
}

#[cfg(not(feature = "local-embeddings"))]
pub(crate) fn resolve_embedding_dimension(_cfg: &AiConfig) -> Option<usize> {
    None
}

/// Build a detected-provider snapshot (synchronous — no I/O).
pub(crate) fn config_snapshot(
    ai_cfg: Option<&AiConfig>,
    embed_cfg: Option<&AiConfig>,
) -> serde_json::Value {
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

/// BL-117 — assemble the resolve_credentials reply. Returns
/// `Value::Null` when no AI chat provider is configured so the
/// caller can branch cleanly without parsing an error string. The
/// api_key is included verbatim because the caller (`nexus-audio`)
/// needs to talk to the same provider endpoint; this is a
/// sensitive payload — the manifest gates dispatch under
/// `ipc.call`, the activity log records each call.
pub(crate) fn resolve_credentials_payload(ai_cfg: Option<&AiConfig>) -> serde_json::Value {
    let Some(cfg) = ai_cfg else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "provider": cfg.provider,
        "api_key": cfg.api_key.clone().unwrap_or_default(),
        "base_url": cfg.base_url,
        "model": cfg.model,
    })
}

// ─── Activity timeline helpers ─────────────────────────────────────────────

/// Default surface tag derivation when the caller doesn't supply one.
/// `mode=complete` defaults to `Complete`; everything else defaults
/// to `Chat`.
pub(crate) fn resolve_surface(explicit: Option<&str>, mode: AiStreamChatMode) -> ActivitySurface {
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
pub(crate) fn last_user_prompt(messages: &[AiStreamAskMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, AiStreamAskRole::User))
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn record_activity_error(
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

// ─── Provider message conversion ───────────────────────────────────────────

/// Translate the typed IPC message list into the provider-facing
/// [`ChatMessage`] shape. A pure projection: same fields, same role
/// names — both serialize as `lowercase`.
pub(crate) fn ipc_messages_to_chat(messages: &[AiStreamAskMessage]) -> Vec<ChatMessage> {
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

/// Translate the legacy `messages` payload (array of `{role, content}`
/// objects) into [`ChatTurn`]s. System messages are dropped here since
/// the provider receives the system prompt via the dedicated `system`
/// arg; assistant text becomes a tool-call-free assistant turn so the
/// model can see its own prior outputs.
pub(crate) fn messages_to_turns(messages: Vec<crate::provider::ChatMessage>) -> Vec<ChatTurn> {
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

/// Phase 5.5 (2c) — convert the rich [`AiChatTurn`] wire form into
/// provider-native [`ChatTurn`]s with the assistant `tool_use` ↔
/// `tool_result` linkage intact. Unlike [`messages_to_turns`] (which
/// can only carry text, dropping the tool calls an assistant issued)
/// this replays a real multi-turn conversation to the provider.
pub(crate) fn ai_turns_to_chat_turns(turns: &[crate::ipc::AiChatTurn]) -> Vec<ChatTurn> {
    use crate::ipc::AiChatTurn;
    use crate::provider::ToolCall;
    turns
        .iter()
        .map(|t| match t {
            AiChatTurn::User { content } => ChatTurn::User {
                content: content.clone(),
            },
            AiChatTurn::Assistant {
                content,
                tool_calls,
            } => ChatTurn::Assistant {
                content: content.clone(),
                tool_calls: tool_calls
                    .iter()
                    .map(|c| ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        input: c.input.clone(),
                    })
                    .collect(),
            },
            AiChatTurn::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => ChatTurn::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: content.clone(),
                is_error: *is_error,
            },
        })
        .collect()
}

// ─── Streaming envelope ─────────────────────────────────────────────────────

/// Shared bus contract for the `stream_chat` family. Owns the
/// `stream_start` / `stream_chunk` / `stream_done` publishes so the
/// chat-loop and complete paths produce byte-identical event streams
/// (modulo content). Any future surface that needs to publish on the
/// same channels must go through this helper.
pub(crate) struct EngineEnvelope {
    pub(crate) ctx: Arc<KernelPluginContext>,
    pub(crate) session_id: String,
    pub(crate) chunk_idx: Arc<AtomicUsize>,
}

impl EngineEnvelope {
    pub(crate) fn new(ctx: Arc<KernelPluginContext>, session_id: String) -> Self {
        Self {
            ctx,
            session_id,
            chunk_idx: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) fn publish_start(&self) {
        let _ = self.ctx.publish(
            "com.nexus.ai.stream_start",
            serde_json::json!({"session_id": &self.session_id}),
        );
    }

    /// Build the per-token sink. The closure is `Send + Sync` so it
    /// can cross provider HTTP boundaries (Anthropic / `OpenAI` adapters
    /// invoke it from inside their streaming task).
    pub(crate) fn chunk_sink(&self) -> impl Fn(String) + Send + Sync + 'static {
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

    /// C27 (#380) — `usage` (provider-reported, when available) rides
    /// along so chat surfaces can show what the call actually cost.
    pub(crate) fn publish_done(&self, text: &str, usage: Option<crate::provider::TokenUsage>) {
        let mut payload = serde_json::json!({"session_id": &self.session_id, "text": text});
        if let Some(u) = usage {
            payload["usage"] = serde_json::json!({
                "input_tokens": u.input_tokens,
                "output_tokens": u.output_tokens,
            });
        }
        let _ = self.ctx.publish("com.nexus.ai.stream_done", payload);
    }
}

// ─── mode=complete post-processing ─────────────────────────────────────────

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
pub(crate) async fn run_complete(
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

    let mut text: String = raw;
    if args.trim == Some(true) {
        let prompt_tail = last_user_tail(messages, 256);
        let stripped = strip_prompt_echo(&text, &prompt_tail).to_string();
        let clipped = trim_to_natural_break(&stripped).to_string();
        text = clipped;
    }
    if let Some(stops) = args.stop.as_deref() {
        text = apply_stop(&text, stops).to_string();
    }
    Ok(text)
}

/// Last `n` chars (by char count, not bytes) of the most recent user
/// message — the heuristic for prompt-echo detection in
/// [`strip_prompt_echo`]. Returns `""` if there is no user message.
pub(crate) fn last_user_tail(messages: &[ChatMessage], n: usize) -> String {
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
pub(crate) fn strip_prompt_echo<'a>(suggested: &'a str, prompt_tail: &str) -> &'a str {
    if prompt_tail.is_empty() {
        return suggested;
    }
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
pub(crate) fn trim_to_natural_break(text: &str) -> &str {
    if text.is_empty() {
        return text;
    }
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
pub(crate) fn apply_stop<'a>(text: &'a str, stops: &[String]) -> &'a str {
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

// ─── Tool dispatch loop ─────────────────────────────────────────────────────

/// Outcome of [`run_tool_dispatch_loop`]: aggregated text + the
/// per-call recording the BL-037 activity timeline needs (tool name,
/// ok/error, file paths the model touched). Files are extracted from
/// the well-known `path` input field used by `read_file` /
/// `write_file` and similar; tools without a `path` arg contribute
/// nothing to `files`.
#[derive(Debug)]
pub(crate) struct ToolDispatchOutcome {
    pub(crate) text: String,
    pub(crate) tool_calls: Vec<nexus_types::activity::ActivityToolCall>,
    pub(crate) files: Vec<String>,
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
pub(crate) async fn run_tool_dispatch_loop(
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
            return Ok(ToolDispatchOutcome {
                text: aggregated,
                tool_calls: tool_calls_recorded,
                files: files_recorded,
            });
        }

        turns.push(ChatTurn::Assistant {
            content: output.text.clone(),
            tool_calls: output.tool_calls.clone(),
        });

        for call in &output.tool_calls {
            let (content, is_error) = execute_tool_call(registry, call).await;
            tool_calls_recorded.push(nexus_types::activity::ActivityToolCall {
                name: call.name.clone(),
                ok: !is_error,
            });
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
pub(crate) async fn execute_tool_call(registry: &ToolRegistry, call: &ToolCall) -> (String, bool) {
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

// ─── JSON helpers ──────────────────────────────────────────────────────────

/// Extract the first top-level JSON array from a model reply, tolerating
/// markdown code fences and surrounding prose. Returns the parsed value
/// only when it deserialises into a `Vec<Value>`.
pub(crate) fn extract_json_array(reply: &str) -> Option<Vec<serde_json::Value>> {
    if let Ok(v) = serde_json::from_str::<Vec<serde_json::Value>>(reply.trim()) {
        return Some(v);
    }
    let bytes = reply.as_bytes();
    let mut start = None;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        let slice = &reply[s..=i];
                        if let Ok(v) = serde_json::from_str::<Vec<serde_json::Value>>(slice) {
                            return Some(v);
                        }
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }
    None
}
