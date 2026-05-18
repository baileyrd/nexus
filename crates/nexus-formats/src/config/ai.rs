//! AI provider and model configuration (`ai.toml`).

use serde::{Deserialize, Serialize};

/// AI provider and model configuration loaded from `.forge/ai.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// Default AI provider name (e.g. `"anthropic"`).
    pub provider: String,
    /// Default model ID.
    pub model: String,
    /// Environment variable that holds the API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Embedding model ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,
    /// Maximum tokens for generation.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f64,
    /// P2-04 — per-provider chat default for `provider = "anthropic"`,
    /// used by the kernel when the runtime `AiConfig.model` is unset.
    /// Falls back to `nexus_ai::anthropic::DEFAULT_MODEL` when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_model: Option<String>,
    /// P2-04 — per-provider chat default for `provider = "openai"`.
    /// Falls back to `nexus_ai::openai::DEFAULT_CHAT_MODEL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_chat_model: Option<String>,
    /// P2-04 — per-provider embedding default for OpenAI. Falls back
    /// to `nexus_ai::openai::DEFAULT_EMBEDDING_MODEL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_embedding_model: Option<String>,
    /// P2-04 — per-provider chat default for `provider = "ollama"`.
    /// Falls back to `nexus_ai::ollama::DEFAULT_CHAT_MODEL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_chat_model: Option<String>,
    /// P2-05 — base URL for the Ollama HTTP API. Falls back to
    /// `nexus_ai::ollama::DEFAULT_BASE_URL` (`http://localhost:11434`).
    /// Distinct from the generic `[providers.<name>].baseUrl` slot so a
    /// forge that defaults `provider = "anthropic"` can still pre-seed
    /// a non-local Ollama endpoint without rewriting the providers map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_base_url: Option<String>,
    /// P2-04 — per-provider embedding default for Ollama. Falls back
    /// to `nexus_ai::ollama::DEFAULT_EMBEDDING_MODEL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_embedding_model: Option<String>,
    /// P2-04 — sampling temperature passed to Ollama's
    /// `/api/generate` (FIM completions). Falls back to
    /// `nexus_ai::ollama::DEFAULT_FIM_TEMPERATURE` (0.2). Lower values
    /// favour deterministic completions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_temperature: Option<f32>,
    /// P2-06 — debounce window for the indexing daemon. Bursts of
    /// `com.nexus.storage.file_*` events within this window collapse
    /// into a single batch. Falls back to
    /// `nexus_ai::indexing_daemon::DEFAULT_DEBOUNCE` (2 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing_debounce_secs: Option<u64>,
    /// Named provider entries from `[providers.<name>]`.
    #[serde(default)]
    pub providers: std::collections::BTreeMap<String, AiProvider>,
    /// Named model entries from `[[models]]` array.
    #[serde(default)]
    pub models: Vec<AiModel>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider:               "anthropic".into(),
            model:                  "claude-sonnet-4-6".into(),
            api_key_env:            Some("ANTHROPIC_API_KEY".into()),
            embedding_model:        None,
            max_tokens:             4096,
            temperature:            0.7,
            anthropic_model:        None,
            openai_chat_model:      None,
            openai_embedding_model: None,
            ollama_chat_model:      None,
            ollama_base_url:        None,
            ollama_embedding_model: None,
            ollama_temperature:     None,
            indexing_debounce_secs: None,
            providers:              std::collections::BTreeMap::new(),
            models:                 Vec::new(),
        }
    }
}

/// A named AI provider entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    /// Provider type (e.g. `"anthropic"`, `"openai"`).
    #[serde(rename = "type")]
    pub provider_type: String,
    /// API key (may contain `${ENV_VAR}` placeholder).
    #[serde(rename = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Base URL override.
    #[serde(rename = "baseUrl", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// A named model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    /// Model identifier.
    pub id: String,
    /// Provider name from `[providers.<name>]`.
    pub provider: String,
    /// Maximum tokens.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Sampling temperature.
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// Optional system prompt override.
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

fn default_max_tokens() -> u32 { 4096 }
fn default_temperature() -> f64 { 0.7 }
