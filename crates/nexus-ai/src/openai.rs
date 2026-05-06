//! `OpenAI` AI and embedding provider implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::embedding::EmbeddingProvider;
use crate::error::AiError;
use crate::provider::{
    AiProvider, ChatMessage, ChatTurn, ChatTurnOutput, Role, ToolCall as ProviderToolCall,
};
use crate::tools::ToolSchema;

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

/// AI provider backed by the `OpenAI` API.
///
/// Implements both [`AiProvider`] (chat) and [`EmbeddingProvider`].
pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    chat_model: String,
    max_tokens: u32,
}

impl OpenAiProvider {
    /// Create a new `OpenAI` provider.
    ///
    /// If `model` is `None`, defaults to `gpt-4o` for chat and
    /// `text-embedding-3-small` for embeddings.
    /// `tls_pinning_enabled` (BL-102) installs the
    /// [`nexus_security::tls`] verifier on the underlying reqwest
    /// client when `true`.
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
            chat_model: model.unwrap_or_else(|| DEFAULT_CHAT_MODEL.to_string()),
            max_tokens,
        }
    }
}

// -- Chat types --

/// Request body for `OpenAI` chat completions.
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<ChatRequestMessage<'a>>,
}

/// A message in an `OpenAI` chat request.
#[derive(Serialize)]
struct ChatRequestMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Response from the `OpenAI` chat completions endpoint.
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

/// Request body for `OpenAI` embeddings.
#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

/// Response from the `OpenAI` embeddings endpoint.
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

    async fn chat_turn_with_tools(
        &self,
        turns: &[ChatTurn],
        system: Option<&str>,
        tools: &[ToolSchema],
        on_chunk: &(dyn Fn(String) + Send + Sync),
    ) -> Result<ChatTurnOutput, AiError> {
        let api_messages = turns_to_openai(turns, system);
        let api_tools: Vec<OpenAiTool<'_>> = tools.iter().map(OpenAiTool::from).collect();

        let body = OpenAiToolRequest {
            model: &self.chat_model,
            max_tokens: self.max_tokens,
            messages: api_messages,
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
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

        let parsed: OpenAiToolResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        parse_openai_response(parsed, on_chunk)
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

// ─── Tool-aware request / response types ────────────────────────────────────

/// Tool-aware request body. Same shape as [`ChatRequest`] but with a
/// richer message variant that can carry assistant `tool_calls` and
/// `role: "tool"` results, plus an optional `tools[]` array.
#[derive(Serialize)]
struct OpenAiToolRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<OpenAiToolMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool<'a>>>,
}

/// One outgoing message. `role` is `"system"` / `"user"` / `"assistant"`
/// / `"tool"`. The other fields are populated per-role; `OpenAI`
/// tolerates `null`s on the wire so we lean on `Option` + `skip_none`.
#[derive(Serialize)]
struct OpenAiToolMessage {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCallOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

/// Outgoing tool-call (echoed back to the model on assistant turns so
/// it can correlate with the prior request).
#[derive(Serialize)]
struct OpenAiToolCallOut {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiFunctionCallOut,
}

#[derive(Serialize)]
struct OpenAiFunctionCallOut {
    name: String,
    /// `OpenAI` carries args as a JSON-encoded string, not a JSON
    /// object. We re-encode the [`ToolCall::input`] `Value` here.
    arguments: String,
}

/// Wire-form for an advertised tool. `OpenAI` wraps tools in a
/// `{type: "function", function: {...}}` envelope.
#[derive(Serialize)]
struct OpenAiTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiFunctionSchema<'a>,
}

#[derive(Serialize)]
struct OpenAiFunctionSchema<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

impl<'a> From<&'a ToolSchema> for OpenAiTool<'a> {
    fn from(s: &'a ToolSchema) -> Self {
        Self {
            kind: "function",
            function: OpenAiFunctionSchema {
                name: &s.name,
                description: &s.description,
                parameters: &s.input_schema,
            },
        }
    }
}

/// Tool-aware response shape.
#[derive(Deserialize, Debug)]
struct OpenAiToolResponse {
    choices: Vec<OpenAiToolChoice>,
}

#[derive(Deserialize, Debug)]
struct OpenAiToolChoice {
    message: OpenAiToolChoiceMessage,
}

#[derive(Deserialize, Debug)]
struct OpenAiToolChoiceMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallIn>>,
}

#[derive(Deserialize, Debug)]
struct OpenAiToolCallIn {
    id: String,
    #[serde(default, rename = "type")]
    _kind: Option<String>,
    function: OpenAiFunctionCallIn,
}

#[derive(Deserialize, Debug)]
struct OpenAiFunctionCallIn {
    name: String,
    /// Arrives as a JSON-encoded string per `OpenAI`'s contract; we
    /// `from_str` it back into a `Value` before returning to the
    /// dispatcher.
    arguments: String,
}

/// Translate cross-provider [`ChatTurn`]s into `OpenAI`'s message list.
/// `system` is prepended as a `role: "system"` message because
/// `OpenAI` doesn't have a top-level system field.
fn turns_to_openai(turns: &[ChatTurn], system: Option<&str>) -> Vec<OpenAiToolMessage> {
    let mut out: Vec<OpenAiToolMessage> = Vec::with_capacity(turns.len() + 1);
    if let Some(sys) = system {
        out.push(OpenAiToolMessage {
            role: "system",
            content: Some(sys.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    for turn in turns {
        match turn {
            ChatTurn::User { content } => {
                out.push(OpenAiToolMessage {
                    role: "user",
                    content: Some(content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            ChatTurn::Assistant {
                content,
                tool_calls,
            } => {
                let calls: Option<Vec<OpenAiToolCallOut>> = if tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls
                            .iter()
                            .map(|c| OpenAiToolCallOut {
                                id: c.id.clone(),
                                kind: "function",
                                function: OpenAiFunctionCallOut {
                                    name: c.name.clone(),
                                    arguments: c.input.to_string(),
                                },
                            })
                            .collect(),
                    )
                };
                out.push(OpenAiToolMessage {
                    role: "assistant",
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content.clone())
                    },
                    tool_calls: calls,
                    tool_call_id: None,
                });
            }
            ChatTurn::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                // `OpenAI` doesn't carry a separate is_error flag; prefix
                // failures so the model can read the signal.
                let body = if *is_error {
                    format!("[error] {content}")
                } else {
                    content.clone()
                };
                out.push(OpenAiToolMessage {
                    role: "tool",
                    content: Some(body),
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id.clone()),
                });
            }
        }
    }
    out
}

/// Decode the `OpenAI` response into a [`ChatTurnOutput`], emitting the
/// text via `on_chunk` and parsing the JSON-string `arguments` for each
/// tool call.
fn parse_openai_response(
    response: OpenAiToolResponse,
    on_chunk: &(dyn Fn(String) + Send + Sync),
) -> Result<ChatTurnOutput, AiError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| AiError::Provider("empty response from OpenAI".to_string()))?;

    let mut text = String::new();
    if let Some(t) = choice.message.content {
        if !t.is_empty() {
            on_chunk(t.clone());
            text = t;
        }
    }

    let mut tool_calls: Vec<ProviderToolCall> = Vec::new();
    if let Some(calls) = choice.message.tool_calls {
        for call in calls {
            let input: serde_json::Value = if call.function.arguments.is_empty() {
                serde_json::Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_str(&call.function.arguments).map_err(|e| {
                    AiError::Serialization(format!(
                        "OpenAI tool_call.arguments not JSON: {e} (raw: {})",
                        call.function.arguments
                    ))
                })?
            };
            tool_calls.push(ProviderToolCall {
                id: call.id,
                name: call.function.name,
                input,
            });
        }
    }

    Ok(ChatTurnOutput { text, tool_calls })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schema_serializes_into_openai_envelope() {
        let schema = ToolSchema {
            name: "read_file".to_string(),
            description: "Read a file.".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let wire = OpenAiTool::from(&schema);
        let json = serde_json::to_value(&wire).expect("ser");
        assert_eq!(json["type"], "function");
        assert_eq!(json["function"]["name"], "read_file");
        assert_eq!(json["function"]["description"], "Read a file.");
        assert_eq!(json["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn user_turn_becomes_user_message() {
        let turns = [ChatTurn::User {
            content: "hi".to_string(),
        }];
        let messages = turns_to_openai(&turns, None);
        assert_eq!(messages.len(), 1);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hi");
    }

    #[test]
    fn system_prepended_when_provided() {
        let turns = [ChatTurn::User {
            content: "hi".to_string(),
        }];
        let messages = turns_to_openai(&turns, Some("be helpful"));
        assert_eq!(messages.len(), 2);
        let sys = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(sys["role"], "system");
        assert_eq!(sys["content"], "be helpful");
    }

    #[test]
    fn assistant_with_tool_call_serializes_arguments_as_string() {
        let turns = [ChatTurn::Assistant {
            content: String::new(),
            tool_calls: vec![ProviderToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "x.md"}),
            }],
        }];
        let messages = turns_to_openai(&turns, None);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["role"], "assistant");
        // content omitted when empty
        assert!(json.get("content").is_none() || json["content"].is_null());
        assert_eq!(json["tool_calls"][0]["id"], "call_1");
        assert_eq!(json["tool_calls"][0]["type"], "function");
        assert_eq!(json["tool_calls"][0]["function"]["name"], "read_file");
        // arguments must be a JSON-encoded string, not an object
        let args = json["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must be a string");
        let decoded: serde_json::Value = serde_json::from_str(args).unwrap();
        assert_eq!(decoded["path"], "x.md");
    }

    #[test]
    fn tool_result_becomes_tool_role_message() {
        let turns = [ChatTurn::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: "file contents".to_string(),
            is_error: false,
        }];
        let messages = turns_to_openai(&turns, None);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "call_1");
        assert_eq!(json["content"], "file contents");
    }

    #[test]
    fn tool_result_error_is_prefixed_for_openai() {
        let turns = [ChatTurn::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: "boom".to_string(),
            is_error: true,
        }];
        let messages = turns_to_openai(&turns, None);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["content"], "[error] boom");
    }

    #[test]
    fn parse_response_decodes_arguments_string_to_value() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_xyz",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"x.md\"}"
                        }
                    }]
                }
            }]
        });
        let parsed: OpenAiToolResponse = serde_json::from_value(body).expect("parse");
        let on_chunk = |_: String| {};
        let out = parse_openai_response(parsed, &on_chunk).expect("parse");
        assert_eq!(out.text, "");
        assert_eq!(out.tool_calls.len(), 1);
        assert_eq!(out.tool_calls[0].id, "call_xyz");
        assert_eq!(out.tool_calls[0].name, "read_file");
        assert_eq!(out.tool_calls[0].input["path"], "x.md");
    }

    #[test]
    fn parse_response_text_only_yields_no_tool_calls() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "All set."
                }
            }]
        });
        let parsed: OpenAiToolResponse = serde_json::from_value(body).expect("parse");
        let chunks = std::sync::Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);
        let out = parse_openai_response(parsed, &on_chunk).expect("parse");
        assert_eq!(out.text, "All set.");
        assert!(out.tool_calls.is_empty());
        assert_eq!(chunks.lock().unwrap().as_slice(), &["All set."]);
    }

    #[test]
    fn parse_response_empty_arguments_decodes_to_empty_object() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_y",
                        "type": "function",
                        "function": {"name": "list_dir", "arguments": ""}
                    }]
                }
            }]
        });
        let parsed: OpenAiToolResponse = serde_json::from_value(body).expect("parse");
        let on_chunk = |_: String| {};
        let out = parse_openai_response(parsed, &on_chunk).expect("parse");
        assert_eq!(out.tool_calls.len(), 1);
        assert!(out.tool_calls[0].input.is_object());
        assert!(out.tool_calls[0].input.as_object().unwrap().is_empty());
    }

    #[test]
    fn parse_response_invalid_arguments_string_returns_error() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_z",
                        "type": "function",
                        "function": {"name": "x", "arguments": "{not json"}
                    }]
                }
            }]
        });
        let parsed: OpenAiToolResponse = serde_json::from_value(body).expect("parse");
        let on_chunk = |_: String| {};
        let err = parse_openai_response(parsed, &on_chunk).expect_err("must fail");
        assert!(matches!(err, AiError::Serialization(_)));
    }
}
