//! Core AI provider trait and shared chat types.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AiError;
use crate::tools::ToolSchema;

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

/// One tool call the model wants the dispatcher to execute. Producer
/// (provider adapter) parses this from the provider-native shape
/// (Anthropic `tool_use` blocks, `OpenAI` `tool_calls[]`); consumer (the
/// dispatch loop in `core_plugin::handle_stream_chat`) hands `name` +
/// `input` to [`crate::tools::ToolRegistry::execute`] and feeds the
/// string result back as a tool-result turn keyed by `id`.
///
/// `OpenAI` returns the tool args as a JSON-encoded string, not a JSON
/// object — the adapter parses it before constructing this struct, so
/// `input` is always a `serde_json::Value` regardless of provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    /// Provider-issued id (Anthropic `toolu_...`, `OpenAI` `call_...`).
    /// The dispatcher echoes this back on the matching tool-result turn
    /// so the model can correlate results with calls when it requested
    /// several at once.
    pub id: String,
    /// Tool name. Must match a key registered in the tool registry or
    /// dispatch surfaces [`crate::tools::ToolError::NotFound`].
    pub name: String,
    /// Decoded arguments. Provider adapters are responsible for
    /// parsing the wire-level shape (Anthropic ships `Value` directly,
    /// `OpenAI` ships a string we `serde_json::from_str` here).
    pub input: serde_json::Value,
}

/// One conversation turn in the tool-aware streaming path. Richer than
/// [`ChatMessage`] because the provider needs to preserve the linkage
/// between an assistant's `tool_use` requests and the subsequent
/// `tool_result` turns.
///
/// Built by `core_plugin::handle_stream_chat` from the incoming
/// `messages` array and from the dispatcher's own bookkeeping after
/// each tool round; consumed by [`AiProvider::chat_turn_with_tools`]
/// which serializes into the provider's native message shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatTurn {
    /// A user message — plain text.
    User {
        /// The user's text.
        content: String,
    },
    /// An assistant message that may include text and/or tool calls.
    /// Empty `content` is fine when the model only emitted tool-use
    /// blocks; empty `tool_calls` is fine when it only emitted text.
    Assistant {
        /// Assistant text, possibly empty.
        content: String,
        /// Tool calls the model issued, in the order returned by the
        /// provider. Empty when the model produced text only.
        tool_calls: Vec<ToolCall>,
    },
    /// The dispatcher's response to a single tool call from the
    /// preceding `Assistant` turn. `tool_use_id` must match the `id` of
    /// the originating `ToolCall`.
    ToolResult {
        /// `ToolCall::id` from the assistant turn this answers.
        tool_use_id: String,
        /// Stringified result the executor returned (or the error
        /// message if `is_error` is set).
        content: String,
        /// `true` when the executor returned a [`crate::tools::ToolError`];
        /// providers surface this distinctly from a successful result.
        is_error: bool,
    },
}

/// Result of a single tool-aware chat turn — what the model produced
/// before either ending the conversation (`tool_calls` empty) or asking
/// the dispatcher to run more tools.
#[derive(Debug, Clone, Default)]
pub struct ChatTurnOutput {
    /// Concatenated text the model emitted in this turn. Already
    /// streamed via the `on_chunk` callback; surfaced here so the
    /// dispatch loop can build the final aggregated response without
    /// re-buffering at the call site.
    pub text: String,
    /// Tool calls the model wants the dispatcher to execute, in the
    /// order returned. Empty signals "I'm done".
    pub tool_calls: Vec<ToolCall>,
}

/// C27 (#380) — provider-reported token usage, accumulated across the
/// calls made through one provider instance. Providers are built
/// per-request by `build_ai_provider`, so an instance's tally spans
/// exactly one logical operation — including every round of a
/// multi-turn tool loop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    /// Tokens the provider billed for the request side (prompt /
    /// input, provider-reported — not estimated).
    pub input_tokens: u64,
    /// Tokens the provider billed for the response side.
    pub output_tokens: u64,
}

impl TokenUsage {
    /// Accumulate another round's usage into this tally.
    pub fn add(&mut self, other: TokenUsage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
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
    async fn chat(&self, messages: &[ChatMessage], system: Option<&str>)
        -> Result<String, AiError>;

    /// C27 (#380) — drain the token usage accumulated by this provider
    /// instance since construction (or the previous drain). `None` for
    /// providers that don't report usage. Call after the last round of
    /// a chat / tool loop to get the operation's total.
    fn take_usage(&self) -> Option<TokenUsage> {
        None
    }

    /// Stream a chat response, calling `on_chunk` with each token as it arrives.
    ///
    /// Returns the complete concatenated response when the stream ends.
    /// The default implementation collects the full response with [`chat`]
    /// and emits a single callback call. Providers that support true streaming
    /// override this to deliver incremental tokens.
    ///
    /// [`chat`]: AiProvider::chat
    async fn chat_stream_with(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        on_chunk: &(dyn Fn(String) + Send + Sync),
    ) -> Result<String, AiError> {
        let text = self.chat(messages, system).await?;
        on_chunk(text.clone());
        Ok(text)
    }

    /// Tool-aware single-turn entry point used by the streaming
    /// dispatch loop in `core_plugin::handle_stream_chat`.
    ///
    /// Sends `turns` (rich enough to round-trip tool-use / tool-result
    /// pairs across providers), advertises `tools` to the model, and
    /// streams text tokens through `on_chunk`. Returns whatever the
    /// model produced this turn — text plus zero or more `ToolCall`s.
    /// The caller is responsible for executing the tools and looping.
    ///
    /// The default implementation falls back to [`chat_stream_with`]
    /// with `tools` and tool-result turns silently dropped, so
    /// providers that don't support function-calling (Ollama until
    /// sub-task 3 lands) keep working as plain chat.
    ///
    /// [`chat_stream_with`]: AiProvider::chat_stream_with
    async fn chat_turn_with_tools(
        &self,
        turns: &[ChatTurn],
        system: Option<&str>,
        _tools: &[ToolSchema],
        on_chunk: &(dyn Fn(String) + Send + Sync),
    ) -> Result<ChatTurnOutput, AiError> {
        let messages = turns_to_legacy_messages(turns);
        let text = self.chat_stream_with(&messages, system, on_chunk).await?;
        Ok(ChatTurnOutput {
            text,
            tool_calls: Vec::new(),
        })
    }

    /// Return the name of the model being used.
    fn model_name(&self) -> &str;
}

/// Lossy projection of [`ChatTurn`] into the legacy text-only
/// [`ChatMessage`] shape, used by the default `chat_turn_with_tools`
/// fallback. Tool-use blocks are dropped; tool-result content is
/// flattened into a `User` message labelled with its `tool_use_id` so
/// the model has at least *some* signal that a result came back.
///
/// Providers that natively understand tool turns (Anthropic, `OpenAI`)
/// override `chat_turn_with_tools` and bypass this entirely.
#[must_use]
pub fn turns_to_legacy_messages(turns: &[ChatTurn]) -> Vec<ChatMessage> {
    turns
        .iter()
        .map(|t| match t {
            ChatTurn::User { content } => ChatMessage {
                role: Role::User,
                content: content.clone(),
            },
            ChatTurn::Assistant { content, .. } => ChatMessage {
                role: Role::Assistant,
                content: content.clone(),
            },
            ChatTurn::ToolResult {
                tool_use_id,
                content,
                ..
            } => ChatMessage {
                role: Role::User,
                content: format!("[tool_result {tool_use_id}]\n{content}"),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_to_legacy_drops_tool_use_blocks() {
        let turns = vec![
            ChatTurn::User {
                content: "hi".to_string(),
            },
            ChatTurn::Assistant {
                content: "calling".to_string(),
                tool_calls: vec![ToolCall {
                    id: "t1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "x"}),
                }],
            },
            ChatTurn::ToolResult {
                tool_use_id: "t1".to_string(),
                content: "contents".to_string(),
                is_error: false,
            },
        ];
        let legacy = turns_to_legacy_messages(&turns);
        assert_eq!(legacy.len(), 3);
        assert_eq!(legacy[0].role, Role::User);
        assert_eq!(legacy[1].role, Role::Assistant);
        assert_eq!(legacy[1].content, "calling");
        assert_eq!(legacy[2].role, Role::User);
        assert!(legacy[2].content.contains("[tool_result t1]"));
        assert!(legacy[2].content.contains("contents"));
    }

    #[test]
    fn tool_call_round_trips_serde() {
        let call = ToolCall {
            id: "toolu_01".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "notes.md"}),
        };
        let s = serde_json::to_string(&call).expect("ser");
        let back: ToolCall = serde_json::from_str(&s).expect("de");
        assert_eq!(back, call);
    }
}
