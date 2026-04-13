//! Core AI provider trait and shared chat types.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AiError;

/// The role of a participant in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System-level instructions.
    System,
    /// User input.
    User,
    /// Assistant response.
    Assistant,
}

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// The role of the message sender.
    pub role: Role,
    /// The text content of the message.
    pub content: String,
}

/// Trait for AI chat completion providers.
///
/// Implementors provide access to a large language model capable of
/// multi-turn chat conversations.
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Send a chat conversation and receive a text response.
    ///
    /// `messages` contains the conversation history. An optional `system`
    /// prompt provides high-level instructions to the model.
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
    ) -> Result<String, AiError>;

    /// Return the name of the model being used.
    fn model_name(&self) -> &str;
}
