//! Anthropic (Claude) AI provider implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AiError;
use crate::provider::{AiProvider, ChatMessage, Role};

/// Default model used by the Anthropic provider.
const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";

/// API endpoint for Anthropic's messages API.
const API_URL: &str = "https://api.anthropic.com/v1/messages";

/// AI provider backed by the Anthropic Messages API.
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider.
    ///
    /// If `model` is `None`, defaults to `claude-sonnet-4-20250514`.
    #[must_use]
    pub fn new(api_key: String, model: Option<String>, max_tokens: u32) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            max_tokens,
        }
    }
}

/// Request body for the Anthropic Messages API.
#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<AnthropicMessage<'a>>,
}

/// A single message in an Anthropic API request.
#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Top-level response from the Anthropic Messages API.
#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

/// A content block in an Anthropic response.
#[derive(Deserialize)]
struct ContentBlock {
    text: String,
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Result<String, AiError> {
        let api_messages: Vec<AnthropicMessage<'_>> = messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| AnthropicMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => unreachable!(),
                },
                content: &m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            system,
            messages: api_messages,
        };

        let response = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if response.status().as_u16() == 401 {
            let text = response.text().await.unwrap_or_default();
            return Err(AiError::AuthFailed(text));
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AiError::Provider(format!("{status}: {text}")));
        }

        let parsed: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        parsed
            .content
            .into_iter()
            .next()
            .map(|block| block.text)
            .ok_or_else(|| AiError::Provider("empty response from Anthropic".to_string()))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
