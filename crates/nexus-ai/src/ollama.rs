//! Ollama (local) AI and embedding provider implementation.

use async_trait::async_trait;
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};

use crate::embedding::EmbeddingProvider;
use crate::error::AiError;
use crate::provider::{
    AiProvider, ChatMessage, ChatTurn, ChatTurnOutput, Role, ToolCall as ProviderToolCall,
};
use crate::tools::ToolSchema;

/// Default base URL for a local Ollama instance. Override via
/// `ai.toml [ai] ollama_base_url = "..."` (P2-05).
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Default chat model. Override via `ai.toml [ai] ollama_chat_model = "..."`
/// (P2-04).
pub const DEFAULT_CHAT_MODEL: &str = "llama3.2";

/// Default embedding model. Override via
/// `ai.toml [ai] ollama_embedding_model = "..."` (P2-04).
pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

/// Default sampling temperature for the FIM `/api/generate` path.
/// Lower values favour deterministic completions (matches editor
/// expectations — the same prefix should yield the same completion
/// across keystrokes). Override via
/// `ai.toml [ai] ollama_temperature = N` (P2-04).
pub const DEFAULT_FIM_TEMPERATURE: f32 = 0.2;

/// Dimensionality of `nomic-embed-text` embeddings.
const EMBEDDING_DIMENSION: usize = 768;

/// AI provider backed by a local or remote Ollama instance.
///
/// Implements both [`AiProvider`] (chat) and [`EmbeddingProvider`].
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    chat_model: String,
    embedding_model: String,
    fim_temperature: f32,
}

impl OllamaProvider {
    /// Create a new Ollama provider.
    ///
    /// `None` for any argument falls back to the corresponding
    /// `DEFAULT_*` constant:
    /// - `base_url`: [`DEFAULT_BASE_URL`]
    /// - `chat_model`: [`DEFAULT_CHAT_MODEL`]
    /// - `embedding_model`: [`DEFAULT_EMBEDDING_MODEL`]
    #[must_use]
    pub fn new(
        base_url: Option<String>,
        chat_model: Option<String>,
        embedding_model: Option<String>,
    ) -> Self {
        Self::with_fim_temperature(base_url, chat_model, embedding_model, None)
    }

    /// `new` plus an explicit FIM temperature override. `None` falls
    /// back to [`DEFAULT_FIM_TEMPERATURE`]. P2-04.
    #[must_use]
    pub fn with_fim_temperature(
        base_url: Option<String>,
        chat_model: Option<String>,
        embedding_model: Option<String>,
        fim_temperature: Option<f32>,
    ) -> Self {
        Self {
            // V4 (`repo-review-2026-06-10.md`): connect timeout only.
            // No read timeout here — a cold Ollama model load holds the
            // response silent for however long the weights take to page
            // in, which legitimately exceeds any reasonable backstop.
            client: reqwest::Client::builder()
                .connect_timeout(nexus_security::tls::OUTBOUND_CONNECT_TIMEOUT)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            chat_model: chat_model.unwrap_or_else(|| DEFAULT_CHAT_MODEL.to_string()),
            embedding_model: embedding_model.unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string()),
            fim_temperature: fim_temperature.unwrap_or(DEFAULT_FIM_TEMPERATURE),
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
    /// Disable thinking mode for Qwen3-family models. Non-thinking models
    /// ignore this field. Without it, Qwen3 may emit only `thinking` chunks
    /// and leave `content` empty, producing a blank response in the UI.
    think: bool,
}

/// A message in an Ollama chat request.
#[derive(Serialize)]
struct OllamaChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Response from the Ollama chat API (non-streaming).
#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
}

/// A single NDJSON line from a streaming Ollama chat response.
#[derive(Deserialize)]
struct OllamaStreamChunk {
    message: OllamaStreamMessage,
    #[serde(default)]
    done: bool,
}

/// The message payload in an Ollama stream chunk.
#[derive(Deserialize)]
struct OllamaStreamMessage {
    #[serde(default)]
    content: String,
}

/// The message payload in an Ollama chat response.
#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

// -- Fill-in-middle (FIM) types — BL-139 --
//
// Ollama's `/api/generate` endpoint accepts a top-level `suffix` field
// for code models that were trained on a FIM objective (codellama,
// qwen2.5-coder, deepseek-coder, starcoder2, …). Models without FIM
// training ignore the suffix gracefully — the response is still the
// model's natural continuation of `prompt`, which is the same shape
// the caller is expecting.

/// Request body for Ollama's `/api/generate` FIM endpoint.
#[derive(Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    /// FIM suffix — what comes AFTER the cursor. Optional because
    /// non-FIM callers can omit it; we always set it for `fim_generate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    suffix: Option<&'a str>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaGenerateOptions>,
}

#[derive(Serialize)]
struct OllamaGenerateOptions {
    /// Hard cap on tokens generated. Maps to llama.cpp's `n_predict`.
    num_predict: u32,
    /// Inference temperature. 0.2 favours determinism, which matches
    /// editor expectations (the same prefix should yield the same
    /// completion across keystrokes).
    temperature: f32,
}

#[derive(Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

/// Outcome of [`OllamaProvider::fim_generate`].
pub struct OllamaFimOutput {
    pub completion: String,
}

impl OllamaProvider {
    /// BL-139 — single-shot fill-in-middle generation against
    /// `/api/generate`. `prefix` is sent as `prompt`; `suffix` rides
    /// in the eponymous field. `max_tokens` caps generation. The
    /// returned string is the model's raw output, untrimmed — the
    /// caller is responsible for stripping any prompt echo or
    /// trailing whitespace that would clash with the existing buffer.
    ///
    /// **Non-FIM fallback.** Ollama returns `400 ... does not support
    /// insert` when the configured model isn't FIM-trained (most
    /// non-coder models, plus several coder models like
    /// `qwen3-coder-next`). On that specific error this helper retries
    /// the request without the `suffix` field, producing a normal
    /// continuation of `prefix`. The ghost-text rendering still works
    /// — the user just loses the suffix-aware completion that FIM
    /// models do best.
    pub async fn fim_generate(
        &self,
        prefix: &str,
        suffix: &str,
        max_tokens: u32,
    ) -> Result<OllamaFimOutput, AiError> {
        match self.run_generate(prefix, Some(suffix), max_tokens).await {
            Err(AiError::Provider(msg)) if is_unsupported_insert_error(&msg) => {
                tracing::debug!(
                    target: "nexus_ai::ollama",
                    model = %self.chat_model,
                    "model does not support FIM insert; retrying without suffix"
                );
                self.run_generate(prefix, None, max_tokens).await
            }
            other => other,
        }
    }

    async fn run_generate(
        &self,
        prefix: &str,
        suffix: Option<&str>,
        max_tokens: u32,
    ) -> Result<OllamaFimOutput, AiError> {
        let url = format!("{}/api/generate", self.base_url);
        let body = OllamaGenerateRequest {
            model: &self.chat_model,
            prompt: prefix,
            suffix,
            stream: false,
            options: Some(OllamaGenerateOptions {
                num_predict: max_tokens,
                temperature: self.fim_temperature,
            }),
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

        let parsed: OllamaGenerateResponse = response
            .json()
            .await
            .map_err(|e| AiError::Serialization(e.to_string()))?;

        Ok(OllamaFimOutput {
            completion: parsed.response,
        })
    }
}

/// True when an Ollama 400 error payload indicates the model can't
/// service the FIM `insert` op. Substring match against the canonical
/// error text — Ollama uses the phrase verbatim across versions.
fn is_unsupported_insert_error(msg: &str) -> bool {
    msg.contains("does not support insert")
}

#[cfg(test)]
mod fim_fallback_tests {
    use super::is_unsupported_insert_error;

    #[test]
    fn detects_canonical_ollama_400_payload() {
        let msg = "400 Bad Request: {\"error\":\"registry.ollama.ai/library/qwen3.5:4b does not support insert\"}";
        assert!(is_unsupported_insert_error(msg));
    }

    #[test]
    fn ignores_unrelated_400_payloads() {
        assert!(!is_unsupported_insert_error(
            "400 Bad Request: {\"error\":\"model not found\"}"
        ));
        assert!(!is_unsupported_insert_error("500 Internal Server Error"));
        assert!(!is_unsupported_insert_error(""));
    }
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
            think: false,
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

    async fn chat_stream_with(
        &self,
        messages: &[ChatMessage],
        system: Option<&str>,
        on_chunk: &(dyn Fn(String) + Send + Sync),
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
            stream: true,
            think: false,
        };

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AiError::Provider(format!("{status}: {text}")));
        }

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut buf: Vec<u8> = Vec::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| AiError::Provider(e.to_string()))?;
            buf.extend_from_slice(&bytes);

            while let Some(newline_pos) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=newline_pos).collect();
                let line = std::str::from_utf8(&line_bytes).unwrap_or("").trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(chunk) = serde_json::from_str::<OllamaStreamChunk>(line) {
                    if !chunk.message.content.is_empty() {
                        full_text.push_str(&chunk.message.content);
                        on_chunk(chunk.message.content);
                    }
                    if chunk.done {
                        break;
                    }
                }
            }
        }

        Ok(full_text)
    }

    async fn chat_turn_with_tools(
        &self,
        turns: &[ChatTurn],
        system: Option<&str>,
        tools: &[ToolSchema],
        on_chunk: &(dyn Fn(String) + Send + Sync),
    ) -> Result<ChatTurnOutput, AiError> {
        let api_messages = turns_to_ollama(turns, system);
        let api_tools: Vec<OllamaTool<'_>> = tools.iter().map(OllamaTool::from).collect();

        let body = OllamaToolRequest {
            model: &self.chat_model,
            messages: api_messages,
            stream: true,
            think: false,
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
        };

        let url = format!("{}/api/chat", self.base_url);
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

        let mut stream = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut text = String::new();
        let mut tool_calls: Vec<ProviderToolCall> = Vec::new();
        let mut next_synth_id: usize = 0;

        let handle_line = |line: &str,
                           text: &mut String,
                           tool_calls: &mut Vec<ProviderToolCall>,
                           next_synth_id: &mut usize|
         -> bool {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return false;
            }
            let Ok(c) = serde_json::from_str::<OllamaToolStreamChunk>(trimmed) else {
                return false;
            };
            if !c.message.content.is_empty() {
                text.push_str(&c.message.content);
                on_chunk(c.message.content);
            }
            if let Some(calls) = c.message.tool_calls {
                for call in calls {
                    let id = call.id.unwrap_or_else(|| {
                        let s = format!("ollama_call_{next_synth_id}");
                        *next_synth_id += 1;
                        s
                    });
                    tool_calls.push(ProviderToolCall {
                        id,
                        name: call.function.name,
                        input: call.function.arguments,
                    });
                }
            }
            c.done
        };

        'outer: while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| AiError::Provider(e.to_string()))?;
            buf.extend_from_slice(&bytes);

            while let Some(newline_pos) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=newline_pos).collect();
                let line = std::str::from_utf8(&line_bytes).unwrap_or("");
                if handle_line(line, &mut text, &mut tool_calls, &mut next_synth_id) {
                    break 'outer;
                }
            }
        }

        // Flush any trailing line that wasn't terminated by `\n`. Real
        // Ollama always sends a final newline, but a server that closes
        // the connection mid-line shouldn't drop the last record.
        if !buf.is_empty() {
            let line = std::str::from_utf8(&buf).unwrap_or("");
            handle_line(line, &mut text, &mut tool_calls, &mut next_synth_id);
        }

        Ok(ChatTurnOutput { text, tool_calls })
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
            model: &self.embedding_model,
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

// ─── Tool-aware request / response types ────────────────────────────────────

/// Tool-aware request body. Same path (`/api/chat`) and stream model as
/// the legacy [`OllamaChatRequest`], but the messages list carries the
/// richer tool-aware shape and an optional `tools[]` array is added.
#[derive(Serialize)]
struct OllamaToolRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaToolMessage>,
    stream: bool,
    think: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool<'a>>>,
}

/// One outgoing message. Roles are `"system"` / `"user"` / `"assistant"`
/// / `"tool"`. Tool-result messages carry `tool_call_id` so the model
/// can match the result back to the originating call.
#[derive(Serialize)]
struct OllamaToolMessage {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCallOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

/// Outgoing tool-call echoed back to the model on assistant turns. Note
/// `arguments` is a JSON object (`Value`), not a JSON-encoded string —
/// that's the key wire-level difference from `OpenAI`'s shape.
#[derive(Serialize)]
struct OllamaToolCallOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    function: OllamaFunctionCallOut,
}

#[derive(Serialize)]
struct OllamaFunctionCallOut {
    name: String,
    arguments: serde_json::Value,
}

/// Wire-form for an advertised tool. Ollama follows the `OpenAI`-style
/// `{type: "function", function: {...}}` envelope.
#[derive(Serialize)]
struct OllamaTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OllamaFunctionSchema<'a>,
}

#[derive(Serialize)]
struct OllamaFunctionSchema<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

impl<'a> From<&'a ToolSchema> for OllamaTool<'a> {
    fn from(s: &'a ToolSchema) -> Self {
        Self {
            kind: "function",
            function: OllamaFunctionSchema {
                name: &s.name,
                description: &s.description,
                parameters: &s.input_schema,
            },
        }
    }
}

/// One NDJSON line from a tool-aware streaming Ollama chat response.
/// Each chunk may carry text, tool calls, or just a `done` flag.
#[derive(Deserialize)]
struct OllamaToolStreamChunk {
    message: OllamaToolStreamMessage,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct OllamaToolStreamMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCallIn>>,
}

/// Incoming tool-call. Older Ollama builds omit `id`; the dispatch loop
/// relies on a stable id to correlate `tool_use_id` on the matching
/// `tool_result` turn, so the streaming reader synthesizes one when
/// missing.
#[derive(Deserialize)]
struct OllamaToolCallIn {
    #[serde(default)]
    id: Option<String>,
    function: OllamaFunctionCallIn,
}

#[derive(Deserialize)]
struct OllamaFunctionCallIn {
    name: String,
    /// Arguments arrive as a JSON object (`Value`), unlike `OpenAI`'s
    /// JSON-encoded string. We forward the value directly to the
    /// dispatcher.
    #[serde(default)]
    arguments: serde_json::Value,
}

/// Translate cross-provider [`ChatTurn`]s into Ollama's message list.
/// Ollama has no top-level `system` field, so a `Some(system)` arg is
/// prepended as a `role: "system"` message — same approach as the
/// `OpenAI` adapter.
fn turns_to_ollama(turns: &[ChatTurn], system: Option<&str>) -> Vec<OllamaToolMessage> {
    let mut out: Vec<OllamaToolMessage> = Vec::with_capacity(turns.len() + 1);
    if let Some(sys) = system {
        out.push(OllamaToolMessage {
            role: "system",
            content: Some(sys.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    for turn in turns {
        match turn {
            ChatTurn::User { content } => {
                out.push(OllamaToolMessage {
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
                let calls: Option<Vec<OllamaToolCallOut>> = if tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls
                            .iter()
                            .map(|c| OllamaToolCallOut {
                                id: Some(c.id.clone()),
                                function: OllamaFunctionCallOut {
                                    name: c.name.clone(),
                                    arguments: c.input.clone(),
                                },
                            })
                            .collect(),
                    )
                };
                out.push(OllamaToolMessage {
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
                // Ollama doesn't model an is_error flag; prefix failures
                // so the model can read the signal (mirrors the `OpenAI`
                // adapter's convention).
                let body = if *is_error {
                    format!("[error] {content}")
                } else {
                    content.clone()
                };
                out.push(OllamaToolMessage {
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

#[cfg(test)]
mod tool_format_tests {
    use super::*;

    #[test]
    fn tool_schema_serializes_into_ollama_envelope() {
        let schema = ToolSchema {
            name: "read_file".to_string(),
            description: "Read a file.".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let wire = OllamaTool::from(&schema);
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
        let messages = turns_to_ollama(&turns, None);
        assert_eq!(messages.len(), 1);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hi");
    }

    #[test]
    fn system_prepended_when_provided() {
        // Ollama has no top-level system field — the adapter must add a
        // synthetic `role: "system"` entry instead.
        let turns = [ChatTurn::User {
            content: "hi".to_string(),
        }];
        let messages = turns_to_ollama(&turns, Some("be helpful"));
        assert_eq!(messages.len(), 2);
        let sys = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(sys["role"], "system");
        assert_eq!(sys["content"], "be helpful");
    }

    #[test]
    fn assistant_with_tool_call_serializes_arguments_as_object() {
        // The wire-level distinction from `OpenAI`: Ollama carries
        // `arguments` as a JSON object, not a JSON-encoded string.
        let turns = [ChatTurn::Assistant {
            content: String::new(),
            tool_calls: vec![ProviderToolCall {
                id: "ollama_call_0".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "x.md"}),
            }],
        }];
        let messages = turns_to_ollama(&turns, None);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["role"], "assistant");
        // content omitted when empty
        assert!(json.get("content").is_none() || json["content"].is_null());
        assert_eq!(json["tool_calls"][0]["id"], "ollama_call_0");
        assert_eq!(json["tool_calls"][0]["function"]["name"], "read_file");
        // arguments must round-trip as a JSON object, not a string
        let args = &json["tool_calls"][0]["function"]["arguments"];
        assert!(args.is_object(), "arguments must be a JSON object");
        assert_eq!(args["path"], "x.md");
    }

    #[test]
    fn assistant_with_text_only_omits_tool_calls() {
        let turns = [ChatTurn::Assistant {
            content: "done".to_string(),
            tool_calls: Vec::new(),
        }];
        let messages = turns_to_ollama(&turns, None);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["content"], "done");
        // Empty tool_calls list must not appear on the wire — Ollama
        // expects the field to be absent in plain assistant turns.
        assert!(json.get("tool_calls").is_none() || json["tool_calls"].is_null());
    }

    #[test]
    fn tool_result_becomes_tool_role_message() {
        let turns = [ChatTurn::ToolResult {
            tool_use_id: "ollama_call_0".to_string(),
            content: "file contents".to_string(),
            is_error: false,
        }];
        let messages = turns_to_ollama(&turns, None);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "ollama_call_0");
        assert_eq!(json["content"], "file contents");
    }

    #[test]
    fn tool_result_error_is_prefixed_for_ollama() {
        // Ollama, like `OpenAI`, lacks an is_error flag — the adapter
        // prefixes failures so the model still sees the signal.
        let turns = [ChatTurn::ToolResult {
            tool_use_id: "ollama_call_0".to_string(),
            content: "boom".to_string(),
            is_error: true,
        }];
        let messages = turns_to_ollama(&turns, None);
        let json = serde_json::to_value(&messages[0]).expect("ser");
        assert_eq!(json["content"], "[error] boom");
    }

    #[test]
    fn parses_streaming_chunk_with_tool_calls() {
        // A single NDJSON chunk that carries a tool call. `arguments`
        // is an object, no `id` (Ollama omits it on most builds).
        let line = serde_json::json!({
            "model": "llama3.2",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "function": {
                        "name": "read_file",
                        "arguments": {"path": "x.md"}
                    }
                }]
            },
            "done": true
        });
        let chunk: OllamaToolStreamChunk =
            serde_json::from_value(line).expect("parse stream chunk");
        assert!(chunk.done);
        assert!(chunk.message.content.is_empty());
        let calls = chunk.message.tool_calls.expect("tool_calls present");
        assert_eq!(calls.len(), 1);
        assert!(calls[0].id.is_none(), "Ollama omits id on most builds");
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[0].function.arguments["path"], "x.md");
    }

    #[test]
    fn parses_streaming_chunk_with_text_only() {
        let line = serde_json::json!({
            "model": "llama3.2",
            "message": {"role": "assistant", "content": "hi"},
            "done": false
        });
        let chunk: OllamaToolStreamChunk =
            serde_json::from_value(line).expect("parse stream chunk");
        assert_eq!(chunk.message.content, "hi");
        assert!(chunk.message.tool_calls.is_none());
        assert!(!chunk.done);
    }

    #[test]
    fn parses_streaming_chunk_keeps_provided_id() {
        // Newer Ollama builds may send a tool-call id; the dispatcher
        // should preserve it instead of synthesizing one.
        let line = serde_json::json!({
            "model": "llama3.2",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "real_id_42",
                    "function": {"name": "n", "arguments": {}}
                }]
            },
            "done": true
        });
        let chunk: OllamaToolStreamChunk =
            serde_json::from_value(line).expect("parse stream chunk");
        let calls = chunk.message.tool_calls.expect("calls");
        assert_eq!(calls[0].id.as_deref(), Some("real_id_42"));
    }

    #[test]
    fn round_trip_assistant_then_tool_result_preserves_id() {
        // The dispatch loop's correlation depends on the synthesized id
        // surviving the round-trip through `turns_to_ollama`.
        let turns = vec![
            ChatTurn::Assistant {
                content: String::new(),
                tool_calls: vec![ProviderToolCall {
                    id: "ollama_call_0".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "x.md"}),
                }],
            },
            ChatTurn::ToolResult {
                tool_use_id: "ollama_call_0".to_string(),
                content: "FILE_BODY".to_string(),
                is_error: false,
            },
        ];
        let messages = turns_to_ollama(&turns, None);
        assert_eq!(messages.len(), 2);
        let assistant = serde_json::to_value(&messages[0]).expect("ser");
        let tool = serde_json::to_value(&messages[1]).expect("ser");
        assert_eq!(assistant["tool_calls"][0]["id"], "ollama_call_0");
        assert_eq!(tool["tool_call_id"], "ollama_call_0");
    }

    #[test]
    fn empty_tool_list_is_omitted_from_request_body() {
        // Probe the request shape via serde — Ollama errors on an empty
        // `tools: []` array on some builds, so we must elide it.
        let body = OllamaToolRequest {
            model: "llama3.2",
            messages: Vec::new(),
            stream: true,
            think: false,
            tools: None,
        };
        let json = serde_json::to_value(&body).expect("ser");
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn populated_tool_list_is_present_on_request_body() {
        let schema = ToolSchema {
            name: "n".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let wire_tools: Vec<OllamaTool<'_>> = std::iter::once(&schema).map(Into::into).collect();
        let body = OllamaToolRequest {
            model: "llama3.2",
            messages: Vec::new(),
            stream: true,
            think: false,
            tools: Some(wire_tools),
        };
        let json = serde_json::to_value(&body).expect("ser");
        let arr = json["tools"].as_array().expect("tools array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["function"]["name"], "n");
    }
}

#[cfg(test)]
mod streaming_dispatch_tests {
    //! End-to-end coverage for `OllamaProvider::chat_turn_with_tools`
    //! using a hand-rolled HTTP server: drives the streaming NDJSON
    //! path that synthesizes tool-call ids and pumps text through
    //! `on_chunk`.
    //!
    //! Kept here (rather than in `core_plugin.rs`) because it exercises
    //! Ollama-specific wire-format quirks that don't appear with the
    //! `ScriptedProvider` used in the dispatch-loop tests.
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::thread;

    /// Spawn a one-shot HTTP server that returns `body` (raw bytes,
    /// already framed for `chunked`/`streaming` responses) once and
    /// shuts down. Returns the bound base URL.
    fn spawn_one_shot_server(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Drain the request line + headers.
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn streams_text_then_yields_tool_calls_with_synthesized_ids() {
        let body = [
            r#"{"model":"llama3.2","message":{"role":"assistant","content":"thinking..."},"done":false}"#,
            r#"{"model":"llama3.2","message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"read_file","arguments":{"path":"x.md"}}}]},"done":true}"#,
        ]
        .join("\n");
        let url = spawn_one_shot_server(body.into_bytes());
        let provider = OllamaProvider::new(Some(url), Some("llama3.2".to_string()), None);

        let chunks = Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);

        let out = provider
            .chat_turn_with_tools(
                &[ChatTurn::User {
                    content: "read x.md".to_string(),
                }],
                None,
                &[ToolSchema {
                    name: "read_file".to_string(),
                    description: "Read a file.".to_string(),
                    input_schema: serde_json::json!({"type": "object"}),
                }],
                &on_chunk,
            )
            .await
            .expect("turn");

        assert_eq!(out.text, "thinking...");
        assert_eq!(chunks.lock().unwrap().as_slice(), &["thinking..."]);
        assert_eq!(out.tool_calls.len(), 1);
        assert_eq!(out.tool_calls[0].name, "read_file");
        assert_eq!(out.tool_calls[0].input["path"], "x.md");
        // Ollama omitted id; the adapter synthesizes a stable one so
        // the dispatch loop can correlate the matching ToolResult turn.
        assert_eq!(out.tool_calls[0].id, "ollama_call_0");
    }

    #[tokio::test]
    async fn streams_text_only_yields_no_tool_calls() {
        let body = [
            r#"{"model":"llama3.2","message":{"role":"assistant","content":"hi "},"done":false}"#,
            r#"{"model":"llama3.2","message":{"role":"assistant","content":"there"},"done":true}"#,
        ]
        .join("\n");
        let url = spawn_one_shot_server(body.into_bytes());
        let provider = OllamaProvider::new(Some(url), Some("llama3.2".to_string()), None);

        let chunks = Mutex::new(Vec::<String>::new());
        let on_chunk = |s: String| chunks.lock().unwrap().push(s);

        let out = provider
            .chat_turn_with_tools(
                &[ChatTurn::User {
                    content: "hi".to_string(),
                }],
                None,
                &[],
                &on_chunk,
            )
            .await
            .expect("turn");

        assert_eq!(out.text, "hi there");
        assert_eq!(chunks.lock().unwrap().as_slice(), &["hi ", "there"]);
        assert!(out.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn http_error_status_surfaces_as_provider_error() {
        // 500 Internal Server Error path — the streaming reader must
        // bail before consuming bytes.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let body = b"upstream model crashed";
                let response = format!(
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
            }
        });
        let url = format!("http://{addr}");
        let provider = OllamaProvider::new(Some(url), Some("llama3.2".to_string()), None);
        let on_chunk = |_: String| {};
        let err = provider
            .chat_turn_with_tools(
                &[ChatTurn::User {
                    content: "x".to_string(),
                }],
                None,
                &[],
                &on_chunk,
            )
            .await
            .expect_err("must error");
        match err {
            AiError::Provider(msg) => assert!(msg.contains("500")),
            other => panic!("expected Provider error, got {other:?}"),
        }
    }
}
