//! AI provider configuration and auto-detection.

use std::env;

use crate::privacy::PrivacyPolicy;

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
        }
    }
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
/// Prefers `OpenAI` (higher-quality embeddings), falls back to Ollama.
///
/// Returns `None` if no embedding provider is detected.
#[must_use]
pub fn detect_embedding_provider() -> Option<AiConfig> {
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
