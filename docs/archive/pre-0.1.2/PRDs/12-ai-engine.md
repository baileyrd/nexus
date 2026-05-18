# PRD: AI Engine Subsystem — Nexus v1.0

**Document:** Product Requirements Document (PRD)  
**Subsystem:** AI Engine (nexus-ai crate)  
**Version:** 1.0  
**Date:** April 2026  
**Status:** 🟢 Shipped — Substantially Complete (see [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md), 2026-04-18)  
**Target Audience:** Core engineering team, plugin developers, integration partners

---

## 1. Executive Summary

The **AI Engine** is the intelligence layer of Nexus — a Rust-based, AI-native developer knowledge environment where AI is a first-class capability integrated at every level. This is not a chat sidebar bolted onto the side of a code editor. Instead, the AI Engine is deeply woven into the Forge, Editor, Terminal, and Database layers, understanding project context, user intent, and development workflows.

The AI Engine operates across **three capability tiers**:
1. **Tier 1 (Inline Assist):** Ghost-text completions, edit suggestions, and context-aware refactoring within the editor.
2. **Tier 2 (Conversational AI):** Multi-turn chat with persistent context, code references, search integration, and database queries.
3. **Tier 3 (Agent Capabilities):** Autonomous multi-step workflows with file I/O, terminal execution, process management, and database manipulation.

The architecture is **provider-agnostic** and **composable**: Anthropic Claude, OpenAI GPT, local models (Ollama, llama.cpp), and custom implementations coexist. The AI Engine exposes Nexus capabilities as structured AI tools, enabling models to reason about files, forge metadata, and terminal output as first-class concepts.

---

## 2. Goals and Success Criteria

### Goals
- Deliver pervasive, context-aware AI capabilities across editor, terminal, and database workflows.
- Enable multiple AI providers simultaneously with transparent fallback and load balancing.
- Provide deterministic, auditable decision-making through multi-step agent workflows.
- Minimize latency (< 50ms to first token for Tier 1) while maintaining high-quality responses.
- Protect user privacy through data minimization and local-first embedding options.

### Success Criteria (v1.0)
- [ ] All three capability tiers functional and tested.
- [ ] 4+ provider implementations available (Anthropic, OpenAI, Ollama, llama.cpp).
- [ ] Context assembly engine handles 5+ context sources with configurable prioritization.
- [ ] Chat surface supports 500+ message history with streaming and conversation branching.
- [ ] Token counting accurate within ±5% of provider's actual token usage.
- [ ] Inline assist latency ≤ 50ms (local model) or ≤ 500ms (cloud with caching).
- [ ] All user data minimization controls functional and auditable.

---

## 3. Architecture Overview

### 3.1 Core Components

```
nexus-ai/
├── providers/
│   ├── trait.rs              # CompletionProvider, ChatProvider, EmbeddingProvider, ToolUseProvider
│   ├── anthropic.rs          # Anthropic Claude implementation
│   ├── openai.rs             # OpenAI GPT implementation
│   ├── ollama.rs             # Local Ollama integration
│   └── llama_cpp.rs          # llama.cpp via HTTP binding
├── context/
│   ├── assembly.rs           # Context window management, source prioritization
│   ├── sources.rs            # Document, terminal, database, git, search context types
│   └── cache.rs              # Embedding and response caching
├── completions/
│   ├── inline.rs             # Ghost text, edit suggestions
│   ├── streaming.rs          # Stream handling, backpressure, cancellation
│   └── tokens.rs             # Token counting, budget allocation
├── chat/
│   ├── conversation.rs       # Conversation storage, history, branching
│   ├── message.rs            # Message types, serialization, signing
│   └── system_prompt.rs      # Prompt templates, forge-specific injection
├── tools/
│   ├── registry.rs           # Tool registration and schema management
│   ├── execution.rs          # Tool invocation, result handling, chains
│   └── functions.rs          # File ops, search, terminal, database tools
├── embeddings/
│   ├── model.rs              # Embedding model abstraction
│   ├── storage.rs            # Vector storage in SQLite (sqlite-vec)
│   └── indexing.rs           # Batch and incremental document embedding
├── router.rs                 # Provider routing, fallback chains, load balancing
├── cache.rs                  # Completion cache, invalidation strategy
├── privacy.rs                # Data classification, minimization, opt-in/out
├── rate_limit.rs             # Provider-specific limits, queuing, retry logic
└── lib.rs                    # Public API facade
```

### 3.2 Component Interactions

```
Editor Keystroke
    ↓
[Inline Assist] → [Context Assembly] → [Token Counter] → [Cache Check]
    ↓
[Provider Router] → [Completion Provider] → [Streaming] → [Ghost Text UI]

User Opens Chat
    ↓
[Chat Surface] → [Conversation Load] → [Context Assembly] → [System Prompt Injection]
    ↓
[Tool Registry] → [Provider Router] → [Chat Provider] → [Tool Executor]
    ↓
[Tool Results] → [Streaming Response] → [Chat UI]
```

---

## 4. Provider Trait System

### 4.1 Core Traits (nexus-ai/providers/trait.rs)

```rust
/// Unified error type for all provider operations.
#[derive(Debug, Clone)]
pub enum ProviderError {
    ApiKeyMissing,
    NetworkError(String),
    RateLimited { retry_after_secs: u64 },
    ContextTooLarge { max_tokens: usize, requested: usize },
    ModelNotAvailable(String),
    InvalidRequest(String),
    InternalError(String),
}

/// Capability flags for runtime feature detection.
#[derive(Debug, Clone, Default)]
pub struct ProviderCapabilities {
    pub supports_streaming: bool,
    pub supports_tool_use: bool,
    pub supports_vision: bool,
    pub supports_embeddings: bool,
    pub max_context_tokens: usize,
    pub supports_parallel_tool_calls: bool,
}

/// Completion request (Tier 1: Inline Assist).
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub prompt: String,
    pub context: Option<String>,
    pub max_tokens: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub stop_sequences: Vec<String>,
    pub stream: bool,
}

/// Streaming completion response chunk.
#[derive(Debug, Clone)]
pub enum CompletionChunk {
    Delta { text: String },
    Complete { stop_reason: StopReason },
    Error(ProviderError),
}

pub enum StopReason {
    StopSequence,
    MaxTokens,
    Unknown,
}

/// Trait for completion providers (stateless, single-turn).
#[async_trait]
pub trait CompletionProvider: Send + Sync {
    /// Complete a prompt without conversation context.
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<String, ProviderError>;

    /// Streaming completion. Returns a channel receiver.
    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<mpsc::UnboundedReceiver<CompletionChunk>, ProviderError>;

    /// Count tokens in a string.
    fn count_tokens(&self, text: &str) -> Result<usize, ProviderError>;

    /// Get provider capabilities (max context, streaming support, etc.).
    fn capabilities(&self) -> ProviderCapabilities;

    /// Provider name for logging, UI selection, cost tracking.
    fn name(&self) -> &str;
}

/// Chat message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMessage {
    System { content: String },
    User { content: String, id: String },
    Assistant { content: String, id: String, tool_calls: Vec<ToolCall> },
    ToolResult { tool_call_id: String, result: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Chat request (Tier 2: Conversational AI).
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub system_prompt: Option<String>,
    pub max_tokens: usize,
    pub temperature: f32,
    pub tool_use: bool,
    pub tools: Option<Vec<ToolSchema>>,
    pub stream: bool,
}

/// Chat response chunk (streaming or complete).
#[derive(Debug, Clone)]
pub enum ChatChunk {
    Delta { text: String, tool_calls: Vec<ToolCall> },
    Complete { stop_reason: StopReason },
    Error(ProviderError),
}

/// Trait for conversational AI providers (multi-turn, context-aware).
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// Send a chat request and get a response.
    async fn chat(&self, request: ChatRequest) -> Result<String, ProviderError>;

    /// Streaming chat response.
    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::UnboundedReceiver<ChatChunk>, ProviderError>;

    /// Count tokens for chat messages.
    fn count_chat_tokens(&self, messages: &[ChatMessage]) -> Result<usize, ProviderError>;

    /// Get provider capabilities.
    fn capabilities(&self) -> ProviderCapabilities;

    fn name(&self) -> &str;
}

/// Embedding request for semantic search.
#[derive(Debug, Clone)]
pub struct EmbeddingRequest {
    pub texts: Vec<String>,
}

/// Embedding response.
#[derive(Debug, Clone)]
pub struct EmbeddingResponse {
    pub embeddings: Vec<Vec<f32>>,
    pub model: String,
}

/// Trait for embedding providers.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of texts.
    async fn embed(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError>;

    /// Embedding dimension (e.g., 1536 for OpenAI).
    fn embedding_dim(&self) -> usize;

    fn name(&self) -> &str;
}

/// Tool schema (JSON Schema) for function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value, // JSON Schema
}

/// Trait for tool-use / agentic capabilities (Tier 3).
#[async_trait]
pub trait ToolUseProvider: ChatProvider {
    /// Execute a tool and return its result as a string.
    async fn execute_tool(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
    ) -> Result<String, ProviderError>;
}
```

### 4.2 Provider Implementation Pattern

Each provider (Anthropic, OpenAI, Ollama) implements the traits with provider-specific logic:

```rust
// anthropic.rs
pub struct AnthropicProvider {
    client: Client,
    model: String,
    api_key: String,
}

#[async_trait]
impl CompletionProvider for AnthropicProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<String, ProviderError> {
        // Call Claude Messages API v2 with proper context assembly
        // Handle streaming flag
        // Return text or error
    }

    fn count_tokens(&self, text: &str) -> Result<usize, ProviderError> {
        // Use Anthropic's token counter (request or internal approximation)
    }
}

#[async_trait]
impl ChatProvider for AnthropicProvider { /* ... */ }
#[async_trait]
impl EmbeddingProvider for AnthropicProvider { /* ... */ }
```

---

## 5. Provider Configuration and Registration

### 5.1 Configuration Model (nexus-ai/config.rs)

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct AIEngineConfig {
    /// Primary provider for inline assist, chat, embeddings.
    pub primary_provider: ProviderConfig,
    
    /// Fallback providers if primary is unavailable.
    pub fallback_providers: Vec<ProviderConfig>,
    
    /// Embedding configuration (local vs cloud, model choice).
    pub embedding_config: EmbeddingConfig,
    
    /// Context window settings.
    pub context_config: ContextConfig,
    
    /// Privacy and data minimization settings.
    pub privacy_config: PrivacyConfig,
    
    /// Rate limiting and quota settings.
    pub rate_limit_config: RateLimitConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub provider_type: ProviderType,
    pub model_id: String,
    pub api_key_source: ApiKeySource, // Keychain, env var, or explicit
    pub base_url: Option<String>, // For local providers
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens_default: usize,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub enum ProviderType {
    Anthropic,
    OpenAI,
    Ollama,
    LlamaCpp,
    Custom(String),
}

#[derive(Debug, Clone)]
pub enum ApiKeySource {
    Keychain(String), // Nexus keychain entry ID
    EnvVar(String),
    Explicit(String),
    None,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    pub provider: ProviderType,
    pub model_id: String,
    pub batch_size: usize,
    pub cache_embeddings: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextConfig {
    pub max_context_tokens: usize,
    pub reserved_tokens_for_response: usize,
    pub source_priorities: HashMap<ContextSourceType, u8>, // 0-10, higher = higher priority
    pub enable_truncation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrivacyConfig {
    pub send_file_contents_to_cloud: bool,
    pub send_terminal_output: bool,
    pub send_database_records: bool,
    pub send_search_results: bool,
    pub local_embedding_only: bool,
    pub anonymize_identifiers: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub tokens_per_day: u64,
    pub queue_max_size: usize,
}
```

### 5.2 Provider Registry and Selection (nexus-ai/providers/registry.rs)

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ChatProvider>>,
    completion_providers: HashMap<String, Arc<dyn CompletionProvider>>,
    embedding_providers: HashMap<String, Arc<dyn EmbeddingProvider>>,
    primary: String,
    fallbacks: Vec<String>,
}

impl ProviderRegistry {
    /// Register a provider by name.
    pub fn register(
        &mut self,
        name: String,
        provider: Arc<dyn ChatProvider>,
    ) {
        self.providers.insert(name, provider);
    }

    /// Get provider by name, with fallback logic.
    pub fn get_provider(&self, name: Option<&str>) -> Option<Arc<dyn ChatProvider>> {
        let target = name.unwrap_or(&self.primary);
        self.providers.get(target).cloned()
    }

    /// Select best provider for a task (cost, latency, capability).
    pub fn select_provider(&self, task: &AITask) -> Arc<dyn ChatProvider> {
        // Route based on:
        // - Task type (completion, chat, tool-use)
        // - Required capabilities (vision, tool-use, etc.)
        // - User preference
        // - Cost constraints
        // - Latency SLO
    }
}

pub enum AITask {
    InlineCompletion,
    Chat,
    Refactoring,
    CodeGeneration,
    DomainExpertise,
}
```

---

## 6. Context Assembly Engine

### 6.1 Context Sources (nexus-ai/context/sources.rs)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContextSourceType {
    CurrentDocument,
    LinkedDocuments,
    OpenTabs,
    GitDiff,
    DatabaseRecords,
    TerminalOutput,
    SearchResults,
    Clipboard,
    ForgeMetadata,
    ProjectReadme,
    RecentMessages, // Chat history
}

#[derive(Debug, Clone)]
pub struct ContextSource {
    pub source_type: ContextSourceType,
    pub content: String,
    pub file_path: Option<String>,
    pub relevance_score: f32, // 0.0-1.0
    pub token_count: usize,
}

// Gather context from various sources
pub async fn gather_context_sources(
    editor: &EditorEngine,
    terminal: &TerminalProcessManager,
    database: &DatabaseEngine,
    user_intent: &str, // From user input or trigger
) -> Result<Vec<ContextSource>, Error> {
    let mut sources = Vec::new();

    // 1. Current document
    if let Some(current_file) = editor.current_file() {
        sources.push(ContextSource {
            source_type: ContextSourceType::CurrentDocument,
            content: editor.get_file_content(&current_file),
            file_path: Some(current_file),
            relevance_score: 1.0,
            token_count: 0, // Filled later
        });
    }

    // 2. Linked documents (cross-references in Forge)
    let linked = editor.get_linked_documents();
    for link in linked {
        sources.push(ContextSource {
            source_type: ContextSourceType::LinkedDocuments,
            content: editor.get_file_content(&link),
            file_path: Some(link),
            relevance_score: 0.8,
            token_count: 0,
        });
    }

    // 3. Open tabs
    for tab in editor.open_tabs() {
        if tab.path != editor.current_file() {
            sources.push(ContextSource {
                source_type: ContextSourceType::OpenTabs,
                content: editor.get_file_content(&tab.path),
                file_path: Some(tab.path),
                relevance_score: 0.6,
                token_count: 0,
            });
        }
    }

    // 4. Git diff
    let diff = terminal.run_sync("git diff --cached")?;
    if !diff.is_empty() {
        sources.push(ContextSource {
            source_type: ContextSourceType::GitDiff,
            content: diff,
            file_path: None,
            relevance_score: 0.9,
            token_count: 0,
        });
    }

    // 5. Terminal output
    let last_output = terminal.get_last_output(1000)?;
    if !last_output.is_empty() {
        sources.push(ContextSource {
            source_type: ContextSourceType::TerminalOutput,
            content: last_output,
            file_path: None,
            relevance_score: 0.7,
            token_count: 0,
        });
    }

    // 6. Recent search results (if user searched recently)
    // ... similar pattern

    Ok(sources)
}
```

### 6.2 Context Window Management (nexus-ai/context/assembly.rs)

```rust
pub struct ContextAssembler {
    max_tokens: usize,
    reserved_for_response: usize,
    source_priorities: HashMap<ContextSourceType, u8>,
    provider: Arc<dyn CompletionProvider>,
}

impl ContextAssembler {
    /// Assemble final context window given available budget.
    pub async fn assemble(
        &self,
        sources: Vec<ContextSource>,
        user_prompt: &str,
    ) -> Result<AssembledContext, Error> {
        let available_tokens = self.max_tokens - self.reserved_for_response;
        let user_prompt_tokens = self.provider.count_tokens(user_prompt)?;
        let mut remaining_tokens = available_tokens - user_prompt_tokens;

        let mut assembled_sources = Vec::new();
        let mut final_context = String::new();

        // Sort sources by priority
        let mut sorted = sources;
        sorted.sort_by(|a, b| {
            let priority_a = self.source_priorities.get(&a.source_type).unwrap_or(&5);
            let priority_b = self.source_priorities.get(&b.source_type).unwrap_or(&5);
            priority_b.cmp(priority_a)
        });

        // Greedily add sources within token budget
        for source in sorted {
            if source.token_count <= remaining_tokens {
                final_context.push_str(&format!(
                    "## {}\n{}\n\n",
                    source.source_type.name(),
                    source.content
                ));
                remaining_tokens -= source.token_count;
                assembled_sources.push(source);
            } else if remaining_tokens > 500 {
                // Truncate source to fit
                let truncated = truncate_by_tokens(
                    &source.content,
                    remaining_tokens,
                    &self.provider,
                )?;
                final_context.push_str(&format!(
                    "## {} (truncated)\n{}\n\n",
                    source.source_type.name(),
                    truncated
                ));
                remaining_tokens = 0;
                break;
            }
        }

        Ok(AssembledContext {
            context: final_context,
            sources: assembled_sources,
            tokens_used: available_tokens - remaining_tokens,
        })
    }
}

pub struct AssembledContext {
    pub context: String,
    pub sources: Vec<ContextSource>,
    pub tokens_used: usize,
}
```

### 6.3 Context Caching (nexus-ai/context/cache.rs)

```rust
pub struct ContextCache {
    cache: Arc<Mutex<LruCache<String, CachedContext>>>,
    ttl_secs: u64,
}

#[derive(Clone)]
struct CachedContext {
    assembled: AssembledContext,
    created_at: std::time::Instant,
}

impl ContextCache {
    /// Get cached context if valid and file contents haven't changed.
    pub fn get(&self, cache_key: &str) -> Option<AssembledContext> {
        let cache = self.cache.lock().unwrap();
        if let Some(cached) = cache.peek(cache_key) {
            if cached.created_at.elapsed().as_secs() < self.ttl_secs {
                return Some(cached.assembled.clone());
            }
        }
        None
    }

    pub fn put(&self, cache_key: String, context: AssembledContext) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(
            cache_key,
            CachedContext {
                assembled: context,
                created_at: std::time::Instant::now(),
            },
        );
    }

    /// Invalidate cache when files change.
    pub fn invalidate_on_change(&self, file_path: &str) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear(); // Simplified; could track file dependencies
    }
}
```

---

## 7. Streaming Architecture

### 7.1 Stream Handling (nexus-ai/completions/streaming.rs)

```rust
use tokio::sync::mpsc;

/// Backpressure-aware streaming response handler.
pub struct StreamHandler {
    rx: mpsc::UnboundedReceiver<CompletionChunk>,
    buffer_size: usize,
}

impl StreamHandler {
    /// Create new stream handler from provider.
    pub fn new(
        provider: &dyn CompletionProvider,
        request: CompletionRequest,
    ) -> Result<Self, ProviderError> {
        let rx = tokio::spawn(async move {
            // Call provider's complete_stream() and return receiver
        });
    }

    /// Collect stream into final response with cancellation support.
    pub async fn collect(
        &mut self,
        on_chunk: Option<Box<dyn Fn(&str) + Send>>,
        cancel_rx: Option<mpsc::UnboundedReceiver<()>>,
    ) -> Result<String, ProviderError> {
        let mut result = String::new();
        let mut cancel = cancel_rx;

        loop {
            tokio::select! {
                Some(chunk) = self.rx.recv() => {
                    match chunk {
                        CompletionChunk::Delta { text } => {
                            result.push_str(&text);
                            if let Some(ref on_chunk) = on_chunk {
                                on_chunk(&text);
                            }
                        }
                        CompletionChunk::Complete { .. } => {
                            return Ok(result);
                        }
                        CompletionChunk::Error(e) => {
                            return Err(e);
                        }
                    }
                }
                Some(_) = cancel.as_mut().map(|c| c.recv()) => {
                    // Cancellation requested
                    return Err(ProviderError::InternalError("Cancelled by user".into()));
                }
                else => break,
            }
        }

        Ok(result)
    }
}

/// Partial response renderer for Tier 1 ghost text.
pub struct PartialResponseRenderer {
    buffer: String,
}

impl PartialResponseRenderer {
    /// Render partial completion as ghost text immediately.
    pub fn render(&mut self, chunk: &str) -> String {
        self.buffer.push_str(chunk);
        // Post UI event to show ghost text
        // Return the rendered ghost text (or empty if not complete line)
        self.buffer.clone()
    }

    /// Finalize and return full completion.
    pub fn finalize(&self) -> String {
        self.buffer.clone()
    }
}
```

---

## 8. Tool System and Function Calling

### 8.1 Tool Registry (nexus-ai/tools/registry.rs)

```rust
pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

pub struct RegisteredTool {
    pub schema: ToolSchema,
    pub executor: Arc<dyn ToolExecutor>,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;
}

impl ToolRegistry {
    /// Register a tool with schema and executor.
    pub fn register(&mut self, name: String, schema: ToolSchema, executor: Arc<dyn ToolExecutor>) {
        self.tools.insert(name, RegisteredTool { schema, executor });
    }

    /// Get all tools as schema array for provider.
    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema.clone()).collect()
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, input: serde_json::Value) -> Result<String, ToolError> {
        let tool = self.tools.get(name).ok_or(ToolError::NotFound)?;
        tool.executor.execute(input).await
    }
}

pub enum ToolError {
    NotFound,
    ExecutionFailed(String),
    InvalidInput(String),
}
```

### 8.2 Built-in Tools (nexus-ai/tools/functions.rs)

```rust
// File operations
pub struct FileReadTool {
    editor: Arc<EditorEngine>,
}

#[async_trait]
impl ToolExecutor for FileReadTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path: String = serde_json::from_value(input["path"].clone())?;
        self.editor.get_file_content(&path)
            .ok_or_else(|| ToolError::NotFound)
    }
}

pub struct FileWriteTool {
    editor: Arc<EditorEngine>,
}

#[async_trait]
impl ToolExecutor for FileWriteTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path: String = serde_json::from_value(input["path"].clone())?;
        let content: String = serde_json::from_value(input["content"].clone())?;
        self.editor.set_file_content(&path, &content)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(format!("Wrote {} bytes to {}", content.len(), path))
    }
}

// Terminal execution
pub struct TerminalExecTool {
    terminal: Arc<TerminalProcessManager>,
}

#[async_trait]
impl ToolExecutor for TerminalExecTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let command: String = serde_json::from_value(input["command"].clone())?;
        self.terminal.run_async(&command).await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }
}

// Database query
pub struct DatabaseQueryTool {
    database: Arc<DatabaseEngine>,
}

#[async_trait]
impl ToolExecutor for DatabaseQueryTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let query: String = serde_json::from_value(input["query"].clone())?;
        let results = self.database.query(&query).await?;
        Ok(serde_json::to_string_pretty(&results)?)
    }
}

// Search forge
pub struct SearchTool {
    editor: Arc<EditorEngine>,
}

#[async_trait]
impl ToolExecutor for SearchTool {
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let query: String = serde_json::from_value(input["query"].clone())?;
        let results = self.editor.search(&query);
        Ok(serde_json::to_string_pretty(&results)?)
    }
}

// Tool schema definitions
pub fn file_read_schema() -> ToolSchema {
    ToolSchema {
        name: "read_file".to_string(),
        description: "Read the contents of a file in the forge.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to file (relative to forge root)"
                }
            },
            "required": ["path"]
        }),
    }
}

pub fn file_write_schema() -> ToolSchema {
    ToolSchema {
        name: "write_file".to_string(),
        description: "Write or modify a file in the forge.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        }),
    }
}

// Multi-step tool chains: execute_then_collect pattern
pub async fn execute_tool_chain(
    registry: &ToolRegistry,
    steps: Vec<ToolCall>,
) -> Result<String, ToolError> {
    let mut results = Vec::new();
    for step in steps {
        let result = registry.execute(&step.name, step.input).await?;
        results.push(result);
    }
    Ok(results.join("\n---\n"))
}
```

---

## 9. Embedding System

### 9.1 Embedding Management (nexus-ai/embeddings/model.rs)

```rust
pub struct EmbeddingModel {
    provider: Arc<dyn EmbeddingProvider>,
    cache: Arc<EmbeddingCache>,
}

pub struct EmbeddingCache {
    vectors: Arc<Mutex<HashMap<String, Vec<f32>>>>,
    metadata: Arc<Mutex<HashMap<String, EmbeddingMetadata>>>,
}

#[derive(Clone)]
pub struct EmbeddingMetadata {
    pub file_path: String,
    pub mtime: u64,
    pub token_count: usize,
}

impl EmbeddingModel {
    /// Embed a single document with caching.
    pub async fn embed_document(
        &self,
        file_path: &str,
        content: &str,
    ) -> Result<Vec<f32>, ProviderError> {
        // Check cache: file_path + mtime
        let cache_key = format!("{}:{}", file_path, mtime);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached);
        }

        // Embed via provider
        let response = self.provider.embed(EmbeddingRequest {
            texts: vec![content.to_string()],
        }).await?;

        // Cache result
        self.cache.put(cache_key, response.embeddings[0].clone());
        Ok(response.embeddings[0].clone())
    }

    /// Embed batch of documents (for initial forge indexing).
    pub async fn embed_batch(
        &self,
        documents: Vec<(String, String)>, // (path, content)
    ) -> Result<Vec<(String, Vec<f32>)>, ProviderError> {
        // Split into chunks respecting provider limits
        // Call provider batch embed
        // Cache all results
        // Return vectors with paths
    }
}
```

### 9.2 Vector Storage (nexus-ai/embeddings/storage.rs)

```rust
/// SQLite-based vector storage using sqlite-vec extension.
pub struct VectorStore {
    db: Arc<rusqlite::Connection>,
    embedding_dim: usize,
}

impl VectorStore {
    /// Initialize vector table.
    pub fn init(&self) -> Result<(), Error> {
        self.db.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS embeddings (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                content TEXT,
                embedding BLOB NOT NULL,
                created_at INTEGER,
                updated_at INTEGER
            );
            
            CREATE INDEX IF NOT EXISTS idx_file_path ON embeddings(file_path);
            "#
        )?;
        Ok(())
    }

    /// Store embedding vector.
    pub fn store(
        &self,
        id: &str,
        file_path: &str,
        content: &str,
        embedding: &[f32],
    ) -> Result<(), Error> {
        let embedding_bytes = embedding.iter()
            .flat_map(|f| f.to_le_bytes())
            .collect::<Vec<_>>();

        self.db.execute(
            "INSERT OR REPLACE INTO embeddings (id, file_path, content, embedding, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, file_path, content, &embedding_bytes, now()],
        )?;
        Ok(())
    }

    /// Semantic search: find similar embeddings.
    pub fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, f32)>, Error> {
        // Use sqlite-vec vec_distance_l2() or vec_distance_cosine()
        let mut stmt = self.db.prepare(
            "SELECT id, vec_distance_cosine(embedding, ?) as distance
             FROM embeddings
             ORDER BY distance
             LIMIT ?"
        )?;

        let query_bytes = query_embedding.iter()
            .flat_map(|f| f.to_le_bytes())
            .collect::<Vec<_>>();

        let results = stmt.query_map(params![&query_bytes, limit as i32], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?))
        })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Delete embeddings for a file (on deletion).
    pub fn delete_file(&self, file_path: &str) -> Result<(), Error> {
        self.db.execute(
            "DELETE FROM embeddings WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(())
    }
}
```

### 9.3 Incremental Indexing (nexus-ai/embeddings/indexing.rs)

```rust
pub struct IncrementalIndexer {
    embedding: Arc<EmbeddingModel>,
    storage: Arc<VectorStore>,
    file_monitor: Arc<FileMonitor>, // From Storage Engine
}

impl IncrementalIndexer {
    /// Watch for file changes and incrementally update embeddings.
    pub async fn watch(&self) -> Result<(), Error> {
        let mut rx = self.file_monitor.subscribe();
        loop {
            match rx.recv().await {
                Some(FileEvent::Created(path)) | FileEvent::Modified(path) => {
                    if should_index(&path) {
                        let content = read_file(&path)?;
                        let embedding = self.embedding.embed_document(&path, &content).await?;
                        self.storage.store(&path, &path, &content, &embedding)?;
                    }
                }
                Some(FileEvent::Deleted(path)) => {
                    self.storage.delete_file(&path)?;
                }
                None => break,
            }
        }
        Ok(())
    }

    /// Initial batch indexing of entire forge.
    pub async fn index_forge(&self, root: &str) -> Result<(), Error> {
        let files = walk_files(root)?;
        for (path, content) in files {
            let embedding = self.embedding.embed_document(&path, &content).await?;
            self.storage.store(&path, &path, &content, &embedding)?;
        }
        Ok(())
    }
}
```

---

## 10. Conversation Management

### 10.1 Conversation Storage (nexus-ai/chat/conversation.rs)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<ChatMessage>,
    pub model_id: String,
    pub provider: String,
    pub metadata: ConversationMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationMetadata {
    pub tags: Vec<String>,
    pub context_sources: Vec<String>,
    pub tokens_used: usize,
    pub cost_usd: f32,
}

/// Conversation manager: CRUD and history.
pub struct ConversationManager {
    db: Arc<DatabaseEngine>,
}

impl ConversationManager {
    /// Create new conversation.
    pub async fn create(
        &self,
        title: &str,
        provider: &str,
        model: &str,
    ) -> Result<Conversation, Error> {
        let conv = Conversation {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            created_at: now(),
            updated_at: now(),
            messages: Vec::new(),
            provider: provider.to_string(),
            model_id: model.to_string(),
            metadata: ConversationMetadata::default(),
        };

        self.db.upsert("conversations", &conv)?;
        Ok(conv)
    }

    /// Load conversation by ID with full history.
    pub async fn load(&self, id: &str) -> Result<Conversation, Error> {
        self.db.get("conversations", id)?
            .ok_or_else(|| Error::NotFound)
    }

    /// Add message to conversation and persist.
    pub async fn add_message(&self, conv_id: &str, message: ChatMessage) -> Result<(), Error> {
        let mut conv = self.load(conv_id).await?;
        conv.messages.push(message);
        conv.updated_at = now();
        self.db.upsert("conversations", &conv)?;
        Ok(())
    }

    /// List all conversations (recent first).
    pub async fn list(&self, limit: usize) -> Result<Vec<Conversation>, Error> {
        self.db.query("SELECT * FROM conversations ORDER BY updated_at DESC LIMIT ?", &[limit])?
    }

    /// Delete conversation.
    pub async fn delete(&self, id: &str) -> Result<(), Error> {
        self.db.delete("conversations", id)
    }

    /// Create branch: copy conversation up to a specific message.
    pub async fn branch(&self, parent_id: &str, branch_at: usize) -> Result<Conversation, Error> {
        let parent = self.load(parent_id).await?;
        let mut branch = parent.clone();
        branch.id = uuid::Uuid::new_v4().to_string();
        branch.messages.truncate(branch_at);
        branch.created_at = now();
        self.db.upsert("conversations", &branch)?;
        Ok(branch)
    }
}
```

### 10.2 History Compaction (nexus-ai/chat/compression.rs)

```rust
/// Compress conversation history using summarization.
pub struct HistoryCompressor {
    summarizer: Arc<dyn ChatProvider>,
}

impl HistoryCompressor {
    /// Compress old messages into a summary message.
    pub async fn compress(
        &self,
        messages: Vec<ChatMessage>,
        summary_threshold: usize,
    ) -> Result<Vec<ChatMessage>, Error> {
        if messages.len() < summary_threshold {
            return Ok(messages);
        }

        let old_messages = &messages[..messages.len() - (summary_threshold / 2)];
        let recent_messages = &messages[messages.len() - (summary_threshold / 2)..];

        // Summarize old messages into a single system message
        let summary_prompt = format!(
            "Summarize this conversation history into key facts and decisions:\n\n{}",
            messages_to_text(old_messages)
        );

        let summary_text = self.summarizer.chat(ChatRequest {
            messages: vec![ChatMessage::User {
                content: summary_prompt,
                id: "compress".to_string(),
            }],
            system_prompt: None,
            max_tokens: 1000,
            temperature: 0.3,
            tool_use: false,
            tools: None,
            stream: false,
        }).await?;

        let mut compressed = vec![
            ChatMessage::System {
                content: format!("Previous conversation summary:\n{}", summary_text),
            }
        ];
        compressed.extend_from_slice(recent_messages);

        Ok(compressed)
    }
}
```

### 10.3 System Prompt Management (nexus-ai/chat/system_prompt.rs)

```rust
pub struct SystemPromptManager {
    templates: HashMap<String, String>,
}

impl SystemPromptManager {
    /// Get system prompt for a task, with forge-specific injection.
    pub fn build_system_prompt(
        &self,
        task: &str,
        forge_context: &ForgeContext,
    ) -> String {
        let template = self.templates.get(task).unwrap_or(&self.templates["default"]);

        // Inject forge-specific facts
        template
            .replace("{{FORGE_NAME}}", &forge_context.name)
            .replace("{{LANGUAGES}}", &forge_context.languages.join(", "))
            .replace("{{MAIN_ENTRY}}", &forge_context.main_entry_point)
            .replace("{{PROJECT_DESCRIPTION}}", &forge_context.description)
    }
}

pub struct ForgeContext {
    pub name: String,
    pub languages: Vec<String>,
    pub main_entry_point: String,
    pub description: String,
}

// Template examples
pub static SYSTEM_PROMPTS: &[(&str, &str)] = &[
    ("default", r#"
You are an AI developer assistant integrated into Nexus, a Rust-based knowledge environment.
You have access to the user's codebase through tools. You can read, write, and execute code.

Forge: {{FORGE_NAME}}
Languages: {{LANGUAGES}}
Main Entry: {{MAIN_ENTRY}}

Always:
- Use context from the forge to make decisions
- Run terminal commands to verify changes
- Reference specific files and line numbers
- Ask for clarification if intent is ambiguous
"#),
    ("refactoring", r#"
You are a code refactoring expert. When refactoring:
- Maintain backward compatibility
- Add tests for critical paths
- Update documentation
- Run tests before finishing
"#),
];
```

---

## 11. Inline Assist Implementation

### 11.1 Ghost Text Completions (nexus-ai/completions/inline.rs)

```rust
pub struct InlineAssist {
    provider: Arc<dyn CompletionProvider>,
    context_assembler: Arc<ContextAssembler>,
    cache: Arc<CompletionCache>,
}

impl InlineAssist {
    /// Trigger inline completion at cursor position.
    pub async fn complete_at_cursor(
        &self,
        editor: &EditorEngine,
        trigger_reason: TriggerReason,
    ) -> Result<InlineCompletion, Error> {
        let cursor = editor.cursor_position()?;
        let prefix = editor.text_before_cursor(cursor, 1000)?;
        let suffix = editor.text_after_cursor(cursor, 500)?;

        // Determine if we should even try
        if !should_trigger(&prefix, &trigger_reason) {
            return Ok(InlineCompletion::NoSuggestion);
        }

        // Assemble minimal context (avoid latency)
        let sources = gather_minimal_context(editor)?;
        let context = self.context_assembler.assemble(sources, &prefix).await?;

        // Check cache
        let cache_key = format!("{}:{}", prefix, context.context);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached);
        }

        // Complete
        let prompt = format!("{}\n\n{}\n<|continue|>", context.context, prefix);
        let completion_request = CompletionRequest {
            prompt,
            context: None,
            max_tokens: 100,
            temperature: 0.5,
            top_p: 0.95,
            stop_sequences: vec!["\n".to_string()],
            stream: false,
        };

        let completion = self.provider.complete(completion_request).await?;

        // Validate completion (no syntax errors in prefix+completion)
        if validate_completion(&prefix, &completion, editor.language()) {
            let result = InlineCompletion::Suggestion {
                text: completion.clone(),
                range: (cursor, cursor),
            };
            self.cache.put(cache_key, result.clone());
            Ok(result)
        } else {
            Ok(InlineCompletion::NoSuggestion)
        }
    }

    /// Multi-suggestion cycling (Ctrl+] for next).
    pub async fn get_next_suggestion(
        &self,
        editor: &EditorEngine,
    ) -> Result<InlineCompletion, Error> {
        // Generate alternative completions and cycle
    }
}

pub enum TriggerReason {
    Manual,         // Ctrl+Space
    AutomaticChar,  // After typing
    AfterWhitespace,
}

pub enum InlineCompletion {
    Suggestion { text: String, range: (usize, usize) },
    NoSuggestion,
}

/// Heuristic: should we try to complete?
fn should_trigger(prefix: &str, reason: &TriggerReason) -> bool {
    match reason {
        TriggerReason::Manual => true,
        TriggerReason::AutomaticChar => {
            // Don't trigger on every keystroke; be selective
            // E.g., after open paren, after type annotation, after assignment
            prefix.ends_with('(') || 
            prefix.ends_with("->") ||
            prefix.ends_with('=')
        }
        TriggerReason::AfterWhitespace => {
            // Trigger after newline or indentation
            prefix.ends_with('\n') || prefix.ends_with("    ")
        }
    }
}
```

### 11.2 Edit Suggestions (nexus-ai/completions/edits.rs)

```rust
pub struct EditSuggester {
    provider: Arc<dyn CompletionProvider>,
}

impl EditSuggester {
    /// Suggest edits to selected text (Refactor, Explain, Fix, etc).
    pub async fn suggest_edit(
        &self,
        text: &str,
        action: EditAction,
    ) -> Result<String, Error> {
        let prompt = match action {
            EditAction::Refactor => {
                format!("Refactor this code for readability and performance:\n\n{}", text)
            }
            EditAction::Explain => {
                format!("Explain what this code does:\n\n{}", text)
            }
            EditAction::Fix => {
                format!("Fix any bugs or inefficiencies in this code:\n\n{}", text)
            }
            EditAction::Translate(lang) => {
                format!("Translate this code to {}:\n\n{}", lang, text)
            }
        };

        let request = CompletionRequest {
            prompt,
            context: None,
            max_tokens: 2000,
            temperature: 0.7,
            top_p: 0.95,
            stop_sequences: vec![],
            stream: false,
        };

        self.provider.complete(request).await
    }
}

pub enum EditAction {
    Refactor,
    Explain,
    Fix,
    Translate(String),
}
```

---

## 12. Token Management

### 12.1 Token Counting (nexus-ai/completions/tokens.rs)

```rust
pub trait TokenCounter {
    fn count_tokens(&self, text: &str) -> Result<usize, Error>;
}

/// Anthropic token counter (internal or via API).
pub struct AnthropicTokenCounter {
    // Use Anthropic's token counter or TikToken approximation
}

#[async_trait]
impl TokenCounter for AnthropicTokenCounter {
    fn count_tokens(&self, text: &str) -> Result<usize, Error> {
        // Anthropic exposes token counting in messages API
        // Or use local approximation: ~4 chars per token
        Ok((text.len() + 3) / 4)
    }
}

/// Token budget management.
pub struct TokenBudget {
    total: usize,
    reserved_for_response: usize,
    allocated: HashMap<ContextSourceType, usize>,
}

impl TokenBudget {
    pub fn new(max_tokens: usize, reserved: usize) -> Self {
        Self {
            total: max_tokens,
            reserved_for_response: reserved,
            allocated: HashMap::new(),
        }
    }

    pub fn remaining(&self) -> usize {
        let used: usize = self.allocated.values().sum();
        self.total - self.reserved_for_response - used
    }

    pub fn allocate(&mut self, source: ContextSourceType, tokens: usize) -> bool {
        if tokens <= self.remaining() {
            self.allocated.insert(source, tokens);
            true
        } else {
            false
        }
    }
}

/// Cost estimation and tracking.
pub struct CostTracker {
    provider_rates: HashMap<String, PricingInfo>,
}

pub struct PricingInfo {
    pub input_per_mtok: f32,   // $ per million input tokens
    pub output_per_mtok: f32,  // $ per million output tokens
}

impl CostTracker {
    pub fn estimate_cost(
        &self,
        provider: &str,
        input_tokens: usize,
        output_tokens: usize,
    ) -> f32 {
        if let Some(pricing) = self.provider_rates.get(provider) {
            let input_cost = (input_tokens as f32 / 1_000_000.0) * pricing.input_per_mtok;
            let output_cost = (output_tokens as f32 / 1_000_000.0) * pricing.output_per_mtok;
            input_cost + output_cost
        } else {
            0.0
        }
    }
}
```

---

## 13. AI Router

### 13.1 Request Routing (nexus-ai/router.rs)

```rust
pub struct AIRouter {
    registry: Arc<ProviderRegistry>,
    model_capabilities: HashMap<String, ModelCapabilities>,
}

pub struct ModelCapabilities {
    pub supports_tool_use: bool,
    pub context_window: usize,
    pub latency_ms: u32,
    pub cost_per_mtok_in: f32,
    pub cost_per_mtok_out: f32,
}

impl AIRouter {
    /// Select best provider for a task.
    pub fn route(
        &self,
        task: &AITask,
        user_preference: Option<&str>,
        constraints: RouteConstraints,
    ) -> Result<RoutedRequest, Error> {
        // 1. If user specified a provider, use it (unless unavailable).
        if let Some(pref) = user_preference {
            let provider = self.registry.get_provider(Some(pref))?;
            return Ok(RoutedRequest { provider: pref.to_string() });
        }

        // 2. If task has specific requirements, filter capable providers.
        let candidates = match task {
            AITask::Refactoring => {
                self.registry.providers_with_capability("formatting")
            }
            AITask::CodeGeneration => {
                self.registry.providers_with_capability("generation")
            }
            AITask::DomainExpertise => {
                // Prefer models fine-tuned for domain knowledge
                self.registry.providers_with_capability("knowledge")
            }
            _ => self.registry.all_providers(),
        };

        // 3. Apply constraints (cost, latency, etc).
        let filtered = candidates.into_iter()
            .filter(|p| {
                self.model_capabilities.get(p)
                    .map(|c| {
                        c.latency_ms <= constraints.max_latency_ms &&
                        c.cost_per_mtok_in <= constraints.max_cost_per_mtok &&
                        c.context_window >= constraints.min_context_tokens
                    })
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        // 4. Score and select best (prefer local models for latency).
        if let Some(best) = filtered.first() {
            Ok(RoutedRequest {
                provider: best.to_string(),
            })
        } else {
            Err(Error::NoCapableProvider)
        }
    }
}

pub struct RouteConstraints {
    pub max_latency_ms: u32,
    pub max_cost_per_mtok: f32,
    pub min_context_tokens: usize,
}

pub struct RoutedRequest {
    pub provider: String,
}
```

---

## 14. Caching Layer

### 14.1 Completion Cache (nexus-ai/cache.rs)

```rust
pub struct CompletionCache {
    cache: Arc<Mutex<LruCache<String, CachedCompletion>>>,
    ttl_secs: u64,
}

#[derive(Clone)]
pub struct CachedCompletion {
    pub text: String,
    pub created_at: Instant,
}

impl CompletionCache {
    pub fn new(max_entries: usize, ttl_secs: u64) -> Self {
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(max_entries).unwrap()
            ))),
            ttl_secs,
        }
    }

    /// Get cached completion if exists and not expired.
    pub fn get(&self, key: &str) -> Option<String> {
        let cache = self.cache.lock().unwrap();
        cache.peek(key).and_then(|cached| {
            if cached.created_at.elapsed().as_secs() < self.ttl_secs {
                Some(cached.text.clone())
            } else {
                None
            }
        })
    }

    pub fn put(&self, key: String, text: String) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(key, CachedCompletion {
            text,
            created_at: Instant::now(),
        });
    }

    /// Invalidate cache when file contents change.
    pub fn invalidate_by_pattern(&self, pattern: &str) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear(); // Simplified; could track dependencies
    }
}
```

---

## 15. Privacy Model

### 15.1 Data Classification (nexus-ai/privacy.rs)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClassification {
    Public,        // Can send to any provider
    Internal,      // Can send to internal/local providers only
    Confidential,  // Never send to cloud providers
}

/// Classify data based on content and user settings.
pub struct DataClassifier {
    privacy_config: PrivacyConfig,
}

impl DataClassifier {
    pub fn classify(&self, source: &ContextSource) -> DataClassification {
        match source.source_type {
            ContextSourceType::CurrentDocument => {
                if self.privacy_config.send_file_contents_to_cloud {
                    DataClassification::Public
                } else {
                    DataClassification::Confidential
                }
            }
            ContextSourceType::TerminalOutput => {
                if self.privacy_config.send_terminal_output {
                    DataClassification::Public
                } else {
                    DataClassification::Confidential
                }
            }
            ContextSourceType::DatabaseRecords => {
                if self.privacy_config.send_database_records {
                    DataClassification::Public
                } else {
                    DataClassification::Confidential
                }
            }
            _ => DataClassification::Internal,
        }
    }

    /// Filter context sources based on classification and provider.
    pub fn filter_for_provider(
        &self,
        sources: Vec<ContextSource>,
        provider: &str,
    ) -> Vec<ContextSource> {
        let is_local = provider == "ollama" || provider == "llama.cpp";

        sources.into_iter()
            .filter(|source| {
                let classification = self.classify(source);
                match (classification, is_local) {
                    (DataClassification::Confidential, false) => false,
                    _ => true,
                }
            })
            .collect()
    }
}

/// Data minimization: anonymize identifiers in context.
pub struct DataAnonymizer {
    patterns: Vec<(Regex, String)>, // Pattern -> replacement
}

impl DataAnonymizer {
    pub fn anonymize(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (pattern, replacement) in &self.patterns {
            result = pattern.replace_all(&result, replacement.as_str()).to_string();
        }
        result
    }
}
```

---

## 16. Rate Limiting

### 16.1 Rate Limit Management (nexus-ai/rate_limit.rs)

```rust
pub struct RateLimiter {
    config: RateLimitConfig,
    state: Arc<Mutex<RateLimitState>>,
}

pub struct RateLimitState {
    requests_this_minute: VecDeque<Instant>,
    tokens_today: usize,
    last_reset: Instant,
}

impl RateLimiter {
    /// Check if request is allowed, queue if at limit.
    pub async fn acquire(&self) -> Result<(), RateLimitError> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();

        // Remove old requests outside 1-minute window
        while let Some(front) = state.requests_this_minute.front() {
            if now.duration_since(*front) > Duration::from_secs(60) {
                state.requests_this_minute.pop_front();
            } else {
                break;
            }
        }

        if state.requests_this_minute.len() >= self.config.requests_per_minute as usize {
            return Err(RateLimitError::RequestsPerMinute);
        }

        state.requests_this_minute.push_back(now);
        Ok(())
    }

    /// Track token usage and reject if daily quota exceeded.
    pub fn track_tokens(&self, tokens: usize) -> Result<(), RateLimitError> {
        let mut state = self.state.lock().unwrap();
        if state.tokens_today + tokens > self.config.tokens_per_day {
            Err(RateLimitError::DailyTokenQuota)
        } else {
            state.tokens_today += tokens;
            Ok(())
        }
    }
}

pub enum RateLimitError {
    RequestsPerMinute,
    DailyTokenQuota,
    Queued, // In queue, will retry
}

/// Request queue with exponential backoff retry.
pub struct RequestQueue {
    queue: Arc<Mutex<VecDeque<QueuedRequest>>>,
    max_size: usize,
}

pub struct QueuedRequest {
    id: String,
    request: CompletionRequest,
    retry_count: u32,
    next_retry: Instant,
}

impl RequestQueue {
    pub fn enqueue(&self, request: CompletionRequest) -> Result<String, Error> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut queue = self.queue.lock().unwrap();

        if queue.len() >= self.max_size {
            return Err(Error::QueueFull);
        }

        queue.push_back(QueuedRequest {
            id: id.clone(),
            request,
            retry_count: 0,
            next_retry: Instant::now(),
        });

        Ok(id)
    }

    pub async fn process(&self, processor: impl Fn(CompletionRequest)) {
        loop {
            let mut queue = self.queue.lock().unwrap();
            if let Some(idx) = queue.iter().position(|r| Instant::now() >= r.next_retry) {
                let req = queue.remove(idx).unwrap();
                processor(req.request);
            } else {
                drop(queue);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
```

---

## 17. Performance Targets

### Service-Level Objectives (SLOs)

| Metric | Target | Notes |
|--------|--------|-------|
| Inline Assist (Tier 1) — Time to First Token | ≤ 50ms | Local models; cloud with caching |
| Inline Assist — Full Completion Time | ≤ 500ms | Streaming display |
| Chat Response Time | ≤ 2s first token | Multi-modal models may be slower |
| Context Assembly | ≤ 100ms | Including token counting |
| Embedding Throughput | ≥ 1000 docs/sec | Batch mode |
| Conversation Load | ≤ 200ms | For 500-message history |
| Tool Execution | Provider-dependent | Sync tools < 5s |
| Token Counting Accuracy | ±5% | vs. provider's actual |

### Load Testing Scenarios
- 100 concurrent inline completions
- 50 concurrent chat sessions
- Batch embedding of 10,000 documents
- Token counting on 1MB of text
- Tool execution chains (10-step workflows)

---

## 18. Chat Surface UX

### 18.1 Chat Interface Components

```
┌─────────────────────────────────────────┐
│  Conversation: "Refactor Auth Module"   │  ← Title + model selector
├─────────────────────────────────────────┤
│  ┌─────────────────────────────────────┐│
│  │ Assistant: Let me look at...         ││
│  │ [💾 read_file] [🔍 search]          ││  ← Tool indicators
│  │ Here's what I see...                 ││
│  │                                      ││
│  │ **Code block:**                      ││
│  │ ┌─ auth.rs ─────────────────────┐   ││
│  │ │ fn validate_token(token) {     │ 📋 ││  ← Copy + Apply buttons
│  │ │   ...                          │ ⚡ ││
│  │ └────────────────────────────────┘   ││
│  │                                      ││
│  │ Shall I apply this change?           ││
│  └─────────────────────────────────────┘│
│  ┌─────────────────────────────────────┐│
│  │ You: Yes, apply it                  ││  ← User message
│  └─────────────────────────────────────┘│
│  ┌─────────────────────────────────────┐│
│  │ Assistant: Applying... ⏳            ││  ← Streaming indicator
│  │ ✓ File updated: src/auth.rs         ││
│  │ ✓ Tests passing: 42/42              ││
│  └─────────────────────────────────────┘│
├─────────────────────────────────────────┤
│  Model: Claude 3.5 Sonnet              │  ← Provider/model picker
│  /refactor /debug /explain /translate  │  ← Quick commands
│  [Type your message...                 ]│  ← Input
│  Ctrl+Enter = Send                     │
└─────────────────────────────────────────┘
```

### 18.2 Conversation History Sidebar

```
Conversations:
─────────────────
• Today
  • Refactor Auth Module          (2h ago)
  • Fix Login Bug                 (4h ago)
  • Database Optimization         (6h ago)
• Yesterday
  • Add Search Feature            (18h ago)
  • Write Tests                   (1d ago)
```

---

## 19. Inline Assist UX

### 19.1 Ghost Text Display

```
// Original
let result = process_data(input);

// With ghost text
let result = process_data(input);  ← 50ms later, ghost text appears
let processed = result.map(|r| {
    println!("{:?}", r);
    r
});
```

**Actions:**
- `Tab` — Accept ghost text
- `Escape` — Dismiss
- `Ctrl+]` — Next suggestion
- `Ctrl+[` — Previous suggestion

### 19.2 Edit Preview

```
┌─────────────────────────────┐
│  Suggested Refactoring      │
├─────────────────────────────┤
│  - Before:                  │
│  ┌──────────────────────┐   │
│  │ if (x > 0) {        │   │
│  │   console.log("x"); │   │
│  │ }                   │   │
│  └──────────────────────┘   │
│                             │
│  + After:                   │
│  ┌──────────────────────┐   │
│  │ if (x > 0) {        │   │
│  │   console.log(`x`); │   │
│  │ }                   │   │
│  └──────────────────────┘   │
│                             │
│  [ Apply ]  [ Dismiss ]     │
└─────────────────────────────┘
```

---

## 20. AI Settings Panel

```
┌────────────────────────────────────────┐
│  AI Engine Settings                    │
├────────────────────────────────────────┤
│                                        │
│  ▸ Provider Configuration              │
│    Primary:  [Claude 3.5 Sonnet    ▼] │
│    Fallback: [GPT-4 Turbo         ▼] │
│                                        │
│  ▸ Context & Privacy                   │
│    ☐ Send file contents to cloud       │
│    ☐ Send terminal output              │
│    ☐ Send database records             │
│    ☐ Use cloud embedding provider      │
│                                        │
│  ▸ Cost Tracking                       │
│    This month: $12.34 / $50.00 budget  │
│    [🔗 Billing]                        │
│                                        │
│  ▸ Rate Limiting                       │
│    Requests/min: 60 / 60               │
│    Tokens/day: 500K / 1M               │
│                                        │
│  ▸ Model Selection                     │
│    Inline Assist:    [Claude 3.5  ▼] │
│    Chat:             [Claude 3.5  ▼] │
│    Embeddings:       [nomic-embed  ▼] │
│                                        │
│  [ Reset Defaults ]  [ Save ]          │
└────────────────────────────────────────┘
```

---

## 21. Error Handling

### Error States & Recovery

| Error | User Message | Recovery |
|-------|--------------|----------|
| API Key Missing | "Authenticate with Anthropic. [Settings]" | Open settings → add key to keychain |
| Rate Limited | "API quota exceeded. Retry in 60s." | Show backoff timer + queue request |
| Model Unavailable | "Claude 3.5 is temporarily unavailable. Using GPT-4..." | Auto-fallback + notify |
| Network Error | "No internet connection. Using local model..." | Fallback to Ollama |
| Context Too Large | "Context window exceeded. Reducing older messages..." | Auto-compress history |
| Tool Failed | "Failed to execute: read_file. Reason: File not found." | Show fix suggestion |

---

## 22. Dependencies and Integration

### Internal Dependencies
- **nexus-kernel:** Event bus, async runtime
- **nexus-editor:** Current file, cursor position, syntax tree
- **nexus-terminal:** Command execution, output capture
- **nexus-database:** Query execution, record retrieval
- **nexus-storage:** File system access, watching
- **nexus-keychain:** Secure API key storage

### External Dependencies
- **anthropic-rs:** Anthropic SDK (pinned to v0.18)
- **openai-rs:** OpenAI SDK
- **tokio:** Async runtime
- **rusqlite:** SQLite with sqlite-vec extension
- **serde_json:** JSON serialization
- **tiktoken-rs:** Token counting (OpenAI)
- **regex:** Pattern matching
- **lru:** LRU cache

---

## 23. Acceptance Criteria

### Tier 1 (Inline Assist)
- [ ] Ghost text completions trigger on demand (Ctrl+Space)
- [ ] Edit suggestions work (refactor, explain, fix, translate)
- [ ] Multi-suggestion cycling works (Ctrl+])
- [ ] Latency ≤ 50ms (local) or ≤ 500ms (cached cloud)

### Tier 2 (Chat)
- [ ] Chat surface renders with streaming messages
- [ ] Tool use integration: can read files, execute terminal commands
- [ ] Conversation persistence: can load 500-message history
- [ ] Conversation branching: can branch at any point
- [ ] Model/provider selector functional

### Tier 3 (Agent)
- [ ] Multi-step tool chains execute end-to-end
- [ ] Tool results feed back into subsequent tool calls
- [ ] Can autonomously refactor a file and run tests

### Providers
- [ ] Anthropic Claude fully integrated
- [ ] OpenAI GPT available as fallback
- [ ] Ollama local integration
- [ ] llama.cpp HTTP binding works

### Context & Privacy
- [ ] Context window budgets enforced
- [ ] Data classification working
- [ ] User can disable cloud transmission
- [ ] Local embedding option available

### Token Management
- [ ] Token counting accurate within ±5%
- [ ] Cost estimation displays correctly
- [ ] Rate limiting enforces quotas
- [ ] Request queue handles backoff

### Performance
- [ ] Inline assist ≤ 50ms first token
- [ ] Chat load time ≤ 2s
- [ ] Embedding throughput ≥ 1000 docs/sec

---

## 24. Implementation Timeline and Phases

### Phase 1: Core Provider Integration (Week 1-2)
- Trait definitions
- Anthropic + OpenAI implementations
- Token counting
- Basic completion

### Phase 2: Context & Chat (Week 2-4)
- Context assembly engine
- Conversation storage & management
- Chat surface UI
- System prompts

### Phase 3: Tools & Agent (Week 4-5)
- Tool registry
- Built-in tool implementations
- Multi-step chains
- Tier 3 agent loop

### Phase 4: Embeddings & Search (Week 5-6)
- Embedding model abstraction
- SQLite vector storage
- Incremental indexing
- Semantic search

### Phase 5: Polish & Performance (Week 6-7)
- Caching layer
- Rate limiting
- Privacy controls
- Error handling
- Performance tuning

### Phase 6: Testing & Docs (Week 7-8)
- Unit tests (90%+ coverage)
- Integration tests
- Performance benchmarks
- Developer documentation

---

## 25. Version History and Rollout

**v1.0 (April 2026)** — Initial release
- All three capability tiers
- 4+ provider implementations
- Context assembly with 5+ sources
- Conversation management
- Inline assist
- Tool system
- Embeddings with SQLite storage

**v1.1 (May 2026)** — Quality & Adoption
- Vision support (Claude's image understanding)
- Advanced prompt engineering templates
- Conversation search
- Cost dashboard

**v1.2 (June 2026)** — Advanced Agent Capabilities
- ReAct-style agentic workflows
- Memory system (persistent facts)
- Recursive task decomposition
- Tool calling optimizations

---

## 26. Conclusion

The AI Engine transforms Nexus from a code editor with an AI chatbot into a genuinely AI-native development environment. By weaving AI capabilities into every layer — inline completions, conversational understanding, autonomous agents — Nexus becomes a true thinking partner for developers.

This PRD provides an implementation roadmap from trait definitions through production deployment, balancing pragmatism (provider diversity, local-first options) with ambition (agent workflows, semantic search, knowledge synthesis).

