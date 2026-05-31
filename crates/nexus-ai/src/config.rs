//! AI provider configuration and auto-detection.

use std::env;

use crate::privacy::PrivacyPolicy;
use crate::sanitize::InjectionPolicy;

/// Configuration for an AI provider.
#[derive(Debug, Clone)]
pub struct AiConfig {
    /// Provider identifier (e.g., "anthropic", "openai", "ollama").
    pub provider: String,
    /// Model name override. When `None`, the provider uses its default.
    pub model: Option<String>,
    /// API key for authenticated providers.
    pub api_key: Option<String>,
    /// Base URL override for self-hosted or proxy endpoints.
    pub base_url: Option<String>,
    /// Maximum number of tokens to generate.
    pub max_tokens: u32,
    /// Total context window of the target model, in tokens. Used by
    /// [`crate::TokenBudget`] when assembling prompts so context plus
    /// reserved response stays under the model's hard limit. Distinct
    /// from [`Self::max_tokens`], which caps generation length only.
    pub context_window: u32,
    /// Tokens reserved out of [`Self::context_window`] for the model's
    /// response. The budget allocator never hands these out to context
    /// sources.
    pub reserved_response_tokens: u32,
    /// PII / secret egress filter policy. Default is
    /// [`PrivacyPolicy::Off`] — opt in by setting this to
    /// [`PrivacyPolicy::Redact`] (or `Strict`) and threading a
    /// [`crate::Redactor`] into the prompt builder.
    pub privacy: PrivacyPolicy,
    /// BL-130 inbound-injection scanner policy for retrieved RAG
    /// chunks. Default is [`InjectionPolicy::Off`] — opt in by
    /// setting this to `Warn` / `Redact` / `Reject` to have the
    /// scanner run alongside the outbound redactor in
    /// [`crate::rag::build_rag_prompt_budgeted`]. Tool-result and
    /// MCP-output wire points are independent of this field (the
    /// scanner API takes a policy argument per call).
    pub injection_policy: InjectionPolicy,
    /// Local embedding model identifier consumed by the
    /// [`crate::LocalEmbedding`] backend (BL-019, ADR 0018). Only read
    /// when [`Self::provider`] is `"local"`. Defaults to
    /// `bge-small-en-v1.5-int8`; see
    /// [`crate::local_embedding::DEFAULT_LOCAL_MODEL`] for the canonical
    /// default and [`crate::local_embedding::map_model`] for accepted
    /// identifiers.
    pub local_embedding_model: Option<String>,
    /// Pin TLS connections to provider endpoints (BL-102). When
    /// `true`, the HTTP client built for `anthropic` / `openai`
    /// providers uses `nexus_security::tls::pinned_client_config`
    /// — every handshake must present a leaf cert whose SHA-256
    /// matches one of the pins in
    /// `nexus_security::tls_pins::HOST_PINS`. Defaults to `false`
    /// because the shipped pin table is empty (operator with
    /// network access seeds it). Sourced from
    /// `KernelConfig::tls_pinning_enabled` at boot.
    pub tls_pinning_enabled: bool,
    /// BL-139 — token cap for the per-keystroke `predict` handler
    /// when the caller omits `max_tokens`. Smaller than
    /// [`Self::max_tokens`] because FIM completions are streamed at
    /// keystroke rate and a runaway response stalls the editor. Set
    /// from `[ai] predict_max_tokens = N` in `ai.toml`; defaults to
    /// 64 tokens.
    pub predict_max_tokens: u32,
    /// P2-04 — per-provider default model override used when
    /// [`Self::model`] is `None`. When both are unset, each provider
    /// falls back to its built-in constant
    /// ([`crate::anthropic::DEFAULT_MODEL`]). Source: `ai.toml`
    /// `[ai] anthropic_model = "..."`.
    pub anthropic_model: Option<String>,
    /// P2-04 — per-provider chat default for `provider = "openai"`.
    /// Falls back to [`crate::openai::DEFAULT_CHAT_MODEL`].
    pub openai_chat_model: Option<String>,
    /// P2-04 — per-provider embedding default for
    /// `embedding.provider = "openai"`. Falls back to
    /// [`crate::openai::DEFAULT_EMBEDDING_MODEL`].
    pub openai_embedding_model: Option<String>,
    /// P2-04 — per-provider chat default for `provider = "ollama"`.
    /// Falls back to [`crate::ollama::DEFAULT_CHAT_MODEL`].
    pub ollama_chat_model: Option<String>,
    /// P2-04 — per-provider embedding default for
    /// `embedding.provider = "ollama"`. Falls back to
    /// [`crate::ollama::DEFAULT_EMBEDDING_MODEL`].
    pub ollama_embedding_model: Option<String>,
    /// P2-04 — temperature passed to Ollama's `/api/generate` (FIM
    /// completions). `None` ⇒ provider default
    /// ([`crate::ollama::DEFAULT_FIM_TEMPERATURE`] = 0.2). Lower
    /// values bias toward deterministic completions, which matches
    /// editor expectations.
    pub ollama_temperature: Option<f32>,
    /// P2-06 — debounce window for the indexing daemon. Bursts of
    /// `com.nexus.storage.file_*` events within this window collapse
    /// into a single batch. `None` ⇒
    /// [`crate::indexing_daemon::DEFAULT_DEBOUNCE`] (2 s). Lower for
    /// snappier reindex; raise for fewer wakeups on a noisy filesystem.
    pub indexing_debounce_secs: Option<u64>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: None,
            api_key: None,
            base_url: None,
            max_tokens: 4096,
            context_window: 8192,
            reserved_response_tokens: 1024,
            privacy: PrivacyPolicy::Off,
            injection_policy: InjectionPolicy::Off,
            local_embedding_model: None,
            tls_pinning_enabled: false,
            predict_max_tokens: 64,
            anthropic_model: None,
            openai_chat_model: None,
            openai_embedding_model: None,
            ollama_chat_model: None,
            ollama_embedding_model: None,
            ollama_temperature: None,
            indexing_debounce_secs: None,
        }
    }
}

impl AiConfig {
    /// P2-06 — resolved indexing-daemon debounce window. Returns
    /// [`crate::indexing_daemon::DEFAULT_DEBOUNCE`] when no override
    /// is configured.
    #[must_use]
    pub fn indexing_debounce(&self) -> std::time::Duration {
        self.indexing_debounce_secs
            .map(std::time::Duration::from_secs)
            .unwrap_or(crate::indexing_daemon::DEFAULT_DEBOUNCE)
    }
}

/// Detect a local-embedding configuration when the
/// `NEXUS_LOCAL_EMBEDDINGS` env var is set to a truthy value
/// (`1`/`true`/`yes`). The optional `NEXUS_LOCAL_EMBEDDING_MODEL` env
/// var overrides the default model identifier.
///
/// Returned [`AiConfig`] has `provider = "local"`; `build_embedding_provider`
/// in `core_plugin.rs` routes that to [`crate::LocalEmbedding`] when
/// the `local-embeddings` feature is on, otherwise returns a clear
/// error.
#[must_use]
pub fn detect_local_embedding() -> Option<AiConfig> {
    let flag = env::var("NEXUS_LOCAL_EMBEDDINGS")
        .ok()?
        .to_ascii_lowercase();
    if !matches!(flag.as_str(), "1" | "true" | "yes" | "on") {
        return None;
    }
    let model = env::var("NEXUS_LOCAL_EMBEDDING_MODEL").ok();
    Some(AiConfig {
        provider: "local".to_string(),
        local_embedding_model: model,
        ..AiConfig::default()
    })
}

/// Detect the best available chat provider from environment variables.
///
/// Checks, in order:
/// 1. `ANTHROPIC_API_KEY` -> Anthropic
/// 2. `OPENAI_API_KEY`    -> `OpenAI`
/// 3. `OLLAMA_BASE_URL`   -> Ollama
///
/// Returns `None` if no provider is detected.
#[must_use]
pub fn detect_provider() -> Option<AiConfig> {
    if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
        return Some(AiConfig {
            provider: "anthropic".to_string(),
            api_key: Some(key),
            ..AiConfig::default()
        });
    }
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        return Some(AiConfig {
            provider: "openai".to_string(),
            api_key: Some(key),
            ..AiConfig::default()
        });
    }
    if let Ok(url) = env::var("OLLAMA_BASE_URL") {
        return Some(AiConfig {
            provider: "ollama".to_string(),
            base_url: Some(url),
            ..AiConfig::default()
        });
    }
    None
}

/// Detect the best available embedding provider from environment variables.
///
/// Order of preference:
/// 1. `NEXUS_LOCAL_EMBEDDINGS=1` -> local fastembed-rs (offline, ADR 0018)
/// 2. `OPENAI_API_KEY`           -> `OpenAI`
/// 3. `OLLAMA_BASE_URL`          -> Ollama
///
/// Returns `None` if no embedding provider is detected.
#[must_use]
pub fn detect_embedding_provider() -> Option<AiConfig> {
    if let Some(local) = detect_local_embedding() {
        return Some(local);
    }
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        return Some(AiConfig {
            provider: "openai".to_string(),
            api_key: Some(key),
            ..AiConfig::default()
        });
    }
    if let Ok(url) = env::var("OLLAMA_BASE_URL") {
        return Some(AiConfig {
            provider: "ollama".to_string(),
            base_url: Some(url),
            ..AiConfig::default()
        });
    }
    None
}
