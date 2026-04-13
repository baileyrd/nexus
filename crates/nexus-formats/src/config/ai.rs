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
            provider:        "anthropic".into(),
            model:           "claude-sonnet-4-6".into(),
            api_key_env:     Some("ANTHROPIC_API_KEY".into()),
            embedding_model: None,
            max_tokens:      4096,
            temperature:     0.7,
            providers:       std::collections::BTreeMap::new(),
            models:          Vec::new(),
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
