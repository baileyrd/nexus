//! OpenAI AI and embedding provider implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::embedding::EmbeddingProvider;
use crate::error::AiError;
use crate::provider::{AiProvider, ChatMessage, Role};

/// Default chat model.
const DEFAULT_CHAT_MODEL: &str = "gpt-4o";

/// Default embedding model.
const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

/// Dimensionality of `text-embedding-3-small` embeddings.
const EMBEDDING_DIMENSION: usize = 1536;

/// Chat completions endpoint.
const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";

/// Embeddings endpoint.
const EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";

/// AI provider backed by the OpenAI API.
///
/// Implements both [`AiProvider`] (chat) and [`EmbeddingProvider`].
pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    chat_model: String,
    max_tokens: u32,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider.
    ///
    /// If `model` is `None`, defaults to `gpt-4o` for chat and
    /// `text-embedding-3-small` for embeddings.
    pub fn new(api_key: String, model: Option<String>, max_tokens: u32) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            chat_model: model.unwrap_or_else(|| DEFAULT_CHAT_MODEL.to_string()),
            max_tokens,
        }
    }
}

// -- Chat types --

/// Request body for OpenAI chat completions.
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<ChatRequestMessage<'a>>,
}

/// A message in an OpenAI chat request.
#[derive(Serialize)]
struct ChatRequestMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Response from the OpenAI chat completions endpoint.
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

/// A single choice in a chat response.
#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

/// The message payload within a chat choice.
#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

// -- Embedding types --

/// Request body for OpenAI embeddings.
#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

/// Response from the OpenAI embeddings endpoint.
#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

/// A single embedding in the response.
#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Result<String, AiError> {
        let mut api_messages: Vec<ChatRequestMessage<'_>> = Vec::new();

        if let Some(sys) = system {
            api_messages.push(ChatRequestMessage {
                role: "system",
                content: sys,
            });
        }

        for msg in messages {
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            api_messages.push(ChatRequestMessage {
                role,
                content: &msg.content,
            });
        }

        let body = ChatRequest {
            model: &self.chat_model,
            max_tokens: self.max_tokens,
            messages: api_messages,
        };

        let response = self
            .client
            .post(CHAT_URL)
            .header("authorization", format!("Bearer {}", self.api_key))
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

        let parsed: ChatResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        parsed
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| AiError::Provider("empty response from OpenAI".to_string()))
    }

    fn model_name(&self) -> &str {
        &self.chat_model
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError> {
        let body = EmbeddingRequest {
            model: DEFAULT_EMBEDDING_MODEL,
            input: texts,
        };

        let response = self
            .client
            .post(EMBEDDINGS_URL)
            .header("authorization", format!("Bearer {}", self.api_key))
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

        let parsed: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }

    fn dimension(&self) -> usize {
        EMBEDDING_DIMENSION
    }
}
