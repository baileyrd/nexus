//! Ollama (local) AI and embedding provider implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::embedding::EmbeddingProvider;
use crate::error::AiError;
use crate::provider::{AiProvider, ChatMessage, Role};

/// Default base URL for a local Ollama instance.
const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Default chat model.
const DEFAULT_CHAT_MODEL: &str = "llama3.2";

/// Default embedding model.
const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

/// Dimensionality of `nomic-embed-text` embeddings.
const EMBEDDING_DIMENSION: usize = 768;

/// AI provider backed by a local or remote Ollama instance.
///
/// Implements both [`AiProvider`] (chat) and [`EmbeddingProvider`].
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    chat_model: String,
}

impl OllamaProvider {
    /// Create a new Ollama provider.
    ///
    /// If `base_url` is `None`, defaults to `http://localhost:11434`.
    /// If `model` is `None`, defaults to `llama3.2` for chat and
    /// `nomic-embed-text` for embeddings.
    #[must_use] 
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            chat_model: model.unwrap_or_else(|| DEFAULT_CHAT_MODEL.to_string()),
        }
    }
}

// -- Chat types --

/// Request body for Ollama chat API.
#[derive(Serialize)]
struct OllamaChatRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaChatMessage<'a>>,
    stream: bool,
}

/// A message in an Ollama chat request.
#[derive(Serialize)]
struct OllamaChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Response from the Ollama chat API.
#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
}

/// The message payload in an Ollama chat response.
#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

// -- Embedding types --

/// Request body for Ollama embed API.
#[derive(Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

/// Response from the Ollama embed API.
#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[async_trait]
impl AiProvider for OllamaProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Result<String, AiError> {
        let mut api_messages: Vec<OllamaChatMessage<'_>> = Vec::new();

        if let Some(sys) = system {
            api_messages.push(OllamaChatMessage {
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
            api_messages.push(OllamaChatMessage {
                role,
                content: &msg.content,
            });
        }

        let url = format!("{}/api/chat", self.base_url);
        let body = OllamaChatRequest {
            model: &self.chat_model,
            messages: api_messages,
            stream: false,
        };

        let response = self
            .client
            .post(&url)
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

        let parsed: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        Ok(parsed.message.content)
    }

    fn model_name(&self) -> &str {
        &self.chat_model
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError> {
        let url = format!("{}/api/embed", self.base_url);
        let body = OllamaEmbedRequest {
            model: DEFAULT_EMBEDDING_MODEL,
            input: texts,
        };

        let response = self
            .client
            .post(&url)
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

        let parsed: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        Ok(parsed.embeddings)
    }

    fn dimension(&self) -> usize {
        EMBEDDING_DIMENSION
    }
}
