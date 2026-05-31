//! Anthropic (Claude) AI provider implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AiError;
use crate::provider::{
    AiProvider, ChatMessage, ChatTurn, ChatTurnOutput, Role, ToolCall as ProviderToolCall,
};
use crate::tools::ToolSchema;

/// Default model used by the Anthropic provider. Override via
/// `ai.toml [ai] anthropic_model = "..."` (P2-04).
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";

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
    /// `tls_pinning_enabled` (BL-102) installs the
    /// [`nexus_security::tls`] verifier on the underlying reqwest
    /// client when `true`; defaults to `false` everywhere via
    /// [`AiConfig::tls_pinning_enabled`].
    #[must_use]
    pub fn new(
        api_key: String,
        model: Option<String>,
        max_tokens: u32,
        tls_pinning_enabled: bool,
    ) -> Self {
        Self {
            client: crate::http_client::build_client(tls_pinning_enabled),
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

    async fn chat_turn_with_tools(
        &self,
        turns: &[ChatTurn],
        system: Option<&str>,
        tools: &[ToolSchema],
        on_chunk: &(dyn Fn(String) + Send + Sync),
    ) -> Result<ChatTurnOutput, AiError> {
        let api_messages = turns_to_anthropic(turns);
        let api_tools: Vec<AnthropicTool<'_>> = tools.iter().map(AnthropicTool::from).collect();

        let body = AnthropicToolRequest {
            model: &self.model,
            max_tokens: self.max_tokens,
            system,
            messages: api_messages,
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
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

        let parsed: AnthropicToolResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        Ok(parse_tool_response(parsed, on_chunk))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

// ─── Tool-aware request/response shapes ─────────────────────────────────────

/// Tool-aware request body. Adds `tools[]` and switches `messages` to
/// the block-array form so we can carry `tool_use` / `tool_result`.
#[derive(Serialize)]
struct AnthropicToolRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<AnthropicBlockMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool<'a>>>,
}

/// Wire-form for a tool advertised to Anthropic. The fields match the
/// `tools[]` element shape on the Messages API.
#[derive(Serialize)]
struct AnthropicTool<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a serde_json::Value,
}

impl<'a> From<&'a ToolSchema> for AnthropicTool<'a> {
    fn from(s: &'a ToolSchema) -> Self {
        Self {
            name: &s.name,
            description: &s.description,
            input_schema: &s.input_schema,
        }
    }
}

/// A message in the block-array request form. `role` is `"user"` or
/// `"assistant"`; `content` is a list of typed blocks.
#[derive(Serialize)]
struct AnthropicBlockMessage {
    role: &'static str,
    content: Vec<AnthropicBlock>,
}

/// One block of content in either direction. Tagged on `type`.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Tool-aware response shape. `content` may interleave `text` and
/// `tool_use` blocks.
#[derive(Deserialize, Debug)]
struct AnthropicToolResponse {
    #[serde(default)]
    content: Vec<AnthropicResponseBlock>,
}

/// One response block. Anthropic includes a `type` discriminator;
/// other fields are present per-variant.
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicResponseBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Forward-compat: tolerate unknown block types (e.g. `thinking`)
    /// rather than 500-ing the whole turn.
    #[serde(other)]
    Unknown,
}

/// Translate cross-provider [`ChatTurn`]s into Anthropic block messages.
///
/// Per Anthropic's spec, system turns belong in the top-level `system`
/// field rather than the messages list, so the `system` arg on
/// [`AiProvider::chat_turn_with_tools`] should be used; if a `User` /
/// `Assistant` / `ToolResult` chain mistakenly contains a system entry
/// the caller is responsible. We don't filter here.
fn turns_to_anthropic(turns: &[ChatTurn]) -> Vec<AnthropicBlockMessage> {
    let mut out: Vec<AnthropicBlockMessage> = Vec::with_capacity(turns.len());

    let mut pending_tool_results: Vec<AnthropicBlock> = Vec::new();

    let flush_tool_results = |out: &mut Vec<AnthropicBlockMessage>,
                              pending: &mut Vec<AnthropicBlock>| {
        if !pending.is_empty() {
            out.push(AnthropicBlockMessage {
                role: "user",
                content: std::mem::take(pending),
            });
        }
    };

    for turn in turns {
        match turn {
            ChatTurn::User { content } => {
                flush_tool_results(&mut out, &mut pending_tool_results);
                out.push(AnthropicBlockMessage {
                    role: "user",
                    content: vec![AnthropicBlock::Text {
                        text: content.clone(),
                    }],
                });
            }
            ChatTurn::Assistant {
                content,
                tool_calls,
            } => {
                flush_tool_results(&mut out, &mut pending_tool_results);
                let mut blocks: Vec<AnthropicBlock> = Vec::new();
                if !content.is_empty() {
                    blocks.push(AnthropicBlock::Text {
                        text: content.clone(),
                    });
                }
                for call in tool_calls {
                    blocks.push(AnthropicBlock::ToolUse {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        input: call.input.clone(),
                    });
                }
                // An assistant turn with neither text nor tool calls
                // would be invalid; skip it rather than send empty content.
                if !blocks.is_empty() {
                    out.push(AnthropicBlockMessage {
                        role: "assistant",
                        content: blocks,
                    });
                }
            }
            ChatTurn::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                // Anthropic expects all tool_result blocks for a given
                // assistant turn to live in a single user message;
                // batch consecutive ToolResult turns.
                pending_tool_results.push(AnthropicBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: if *is_error { Some(true) } else { None },
                });
            }
        }
    }
    flush_tool_results(&mut out, &mut pending_tool_results);
    out
}

/// Decode the model's response into a [`ChatTurnOutput`], emitting each
/// text block through `on_chunk` so the streaming UI sees tokens
/// arrive even though we're using the non-streaming endpoint.
fn parse_tool_response(
    response: AnthropicToolResponse,
    on_chunk: &(dyn Fn(String) + Send + Sync),
) -> ChatTurnOutput {
    let mut text = String::new();
    let mut tool_calls: Vec<ProviderToolCall> = Vec::new();

    for block in response.content {
        match block {
            AnthropicResponseBlock::Text { text: t } => {
                if !t.is_empty() {
                    on_chunk(t.clone());
                    text.push_str(&t);
                }
            }
            AnthropicResponseBlock::ToolUse { id, name, input } => {
                tool_calls.push(ProviderToolCall { id, name, input });
            }
            AnthropicResponseBlock::Unknown => {}
        }
    }

    ChatTurnOutput { text, tool_calls }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schema_serializes_into_anthropic_shape() {
        let schema = ToolSchema {
            name: "read_file".to_string(),
            description: "Read a file.".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let wire = AnthropicTool::from(&schema);
        let json = serde_json::to_value(&wire).expect("serialize tool");
        assert_eq!(json["name"], "read_file");
        assert_eq!(json["description"], "Read a file.");
        assert_eq!(json["input_schema"]["type"], "object");
    }

    #[test]
    fn user_turn_becomes_text_block() {
        let turns = [ChatTurn::User {
            content: "hi".to_string(),
        }];
        let messages = turns_to_anthropic(&turns);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        let json = serde_json::to_value(&messages[0].content).expect("ser");
        assert_eq!(json[0]["type"], "text");
        assert_eq!(json[0]["text"], "hi");
    }

    #[test]
    fn assistant_with_tool_call_keeps_text_then_tool_use() {
        let turns = [ChatTurn::Assistant {
            content: "let me check".to_string(),
            tool_calls: vec![ProviderToolCall {
                id: "toolu_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "x.md"}),
            }],
        }];
        let messages = turns_to_anthropic(&turns);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        let json = serde_json::to_value(&messages[0].content).expect("ser");
        assert_eq!(json.as_array().unwrap().len(), 2);
        assert_eq!(json[0]["type"], "text");
        assert_eq!(json[0]["text"], "let me check");
        assert_eq!(json[1]["type"], "tool_use");
        assert_eq!(json[1]["id"], "toolu_1");
        assert_eq!(json[1]["name"], "read_file");
        assert_eq!(json[1]["input"]["path"], "x.md");
    }

    #[test]
    fn consecutive_tool_results_collapse_into_one_user_message() {
        let turns = [
            ChatTurn::ToolResult {
                tool_use_id: "t1".to_string(),
                content: "alpha".to_string(),
                is_error: false,
            },
            ChatTurn::ToolResult {
                tool_use_id: "t2".to_string(),
                content: "beta".to_string(),
                is_error: true,
            },
        ];
        let messages = turns_to_anthropic(&turns);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        let json = serde_json::to_value(&messages[0].content).expect("ser");
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "tool_result");
        assert_eq!(arr[0]["tool_use_id"], "t1");
        assert_eq!(arr[0]["content"], "alpha");
        assert!(arr[0].get("is_error").is_none()); // success → omitted
        assert_eq!(arr[1]["tool_use_id"], "t2");
        assert_eq!(arr[1]["is_error"], true);
    }

    #[test]
    fn assistant_with_only_text_has_no_tool_use_block() {
        let turns = [ChatTurn::Assistant {
            content: "done".to_string(),
            tool_calls: vec![],
        }];
        let messages = turns_to_anthropic(&turns);
        assert_eq!(messages.len(), 1);
        let json = serde_json::to_value(&messages[0].content).expect("ser");
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["type"], "text");
    }

    #[test]
    fn assistant_with_only_tool_call_has_no_text_block() {
        let turns = [ChatTurn::Assistant {
            content: String::new(),
            tool_calls: vec![ProviderToolCall {
                id: "t".to_string(),
                name: "n".to_string(),
                input: serde_json::Value::Null,
            }],
        }];
        let messages = turns_to_anthropic(&turns);
        let json = serde_json::to_value(&messages[0].content).expect("ser");
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["type"], "tool_use");
    }

    #[test]
    fn parse_response_collects_text_and_tool_use() {
        let body = serde_json::json!({
            "content": [
                {"type": "text", "text": "I'll read it."},
                {"type": "tool_use", "id": "toolu_abc", "name": "read_file", "input": {"path": "x.md"}}
            ]
        });
        let parsed: AnthropicToolResponse = serde_json::from_value(body).expect("parse");
        let chunks = std::sync::Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);
        let out = parse_tool_response(parsed, &on_chunk);
        assert_eq!(out.text, "I'll read it.");
        assert_eq!(out.tool_calls.len(), 1);
        assert_eq!(out.tool_calls[0].id, "toolu_abc");
        assert_eq!(out.tool_calls[0].name, "read_file");
        assert_eq!(out.tool_calls[0].input["path"], "x.md");
        assert_eq!(chunks.lock().unwrap().as_slice(), &["I'll read it."]);
    }

    #[test]
    fn parse_response_text_only_yields_no_tool_calls() {
        let body = serde_json::json!({
            "content": [
                {"type": "text", "text": "All set."}
            ]
        });
        let parsed: AnthropicToolResponse = serde_json::from_value(body).expect("parse");
        let on_chunk = |_: String| {};
        let out = parse_tool_response(parsed, &on_chunk);
        assert_eq!(out.text, "All set.");
        assert!(out.tool_calls.is_empty());
    }

    #[test]
    fn parse_response_tolerates_unknown_block_type() {
        // Anthropic adds new block types over time (e.g. `thinking`);
        // the adapter must skip them rather than fail the whole turn.
        let body = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "ok"}
            ]
        });
        let parsed: AnthropicToolResponse = serde_json::from_value(body).expect("parse");
        let on_chunk = |_: String| {};
        let out = parse_tool_response(parsed, &on_chunk);
        assert_eq!(out.text, "ok");
    }
}
