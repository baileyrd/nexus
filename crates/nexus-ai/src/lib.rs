//! Nexus AI engine: provider traits, embeddings, and RAG pipeline.
//!
//! This crate holds the AI plugin's internals — chat provider traits
//! (Anthropic, `OpenAI`, Ollama), embedding providers, chunker, and the
//! retrieval-augmented generation pipeline. It does **not** touch
//! `SQLite`; vector storage goes through `com.nexus.storage` IPC.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod ipc;

pub mod activity_log;
mod anthropic;
mod chunker;
mod config;
pub mod core_plugin;
mod embedding;
// C28 (#381) — .aiignore + frontmatter AI exclusion.
pub mod exclusion;
pub mod enrichment;
mod error;
/// BL-116 — `com.nexus.ai::generate_docs` implementation. Lives in
/// its own module so the prompt-building + doc-comment-wrapping
/// logic stays unit-testable without firing a real provider.
mod generate_docs;
mod handlers;
mod http_client;
pub mod indexing_daemon;
#[cfg(feature = "local-embeddings")]
pub mod local_embedding;
mod ollama;
mod openai;
pub mod privacy;
mod provider;
mod rag;
pub mod sanitize;
mod tokens;
pub mod tools;
mod vectorstore;

pub use activity_log::{ActivityRecorder, ACTIVITY_LOG_PATH};
// BL-052 — activity types live in `nexus-types::activity` so non-AI
// emitters (terminal, git, storage, etc.) can publish without
// depending on `nexus-ai`. Re-exported here so existing call sites
// (`use nexus_ai::ActivityEntry`) keep compiling.
pub use anthropic::AnthropicProvider;
pub use chunker::{chunks_from_blocks, Chunk};
pub use config::{detect_embedding_provider, detect_local_embedding, detect_provider, AiConfig};
pub use core_plugin::AiCorePlugin;
pub use embedding::EmbeddingProvider;
pub use enrichment::{body_hash, merge_frontmatter, strip_frontmatter, EnrichmentProposal};
pub use error::AiError;
#[cfg(feature = "local-embeddings")]
pub use local_embedding::{
    dimension_for as local_embedding_dimension_for, LocalEmbedding, BATCH_CACHE_BYPASS_THRESHOLD,
    DEFAULT_CACHE_MAX_ENTRIES, DEFAULT_LOCAL_MODEL,
};
pub use nexus_types::activity::{
    ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface, ActivityToolCall,
    ACTIVITY_APPENDED_TOPIC, AI_ACTIVITY_APPENDED_TOPIC,
};
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use privacy::{PrivacyPolicy, Redaction, Redactor};
pub use provider::{AiProvider, ChatMessage, ChatTurn, ChatTurnOutput, Role, ToolCall};
pub use rag::{index_file as rag_index_file, query as rag_query, Citation, RagResponse};
pub use sanitize::{Finding, InjectionPolicy, InjectionSource, ScanResult, Scanner};
pub use tokens::{ApproxTokenCounter, BudgetWarning, ContextSourceKind, TokenBudget, TokenCounter};
pub use tools::{
    read_file_schema, register_storage_builtins, write_file_schema, ReadFileTool, RegisteredTool,
    ToolError, ToolExecutor, ToolRegistry, ToolSchema, WriteFileTool,
};
pub use vectorstore::{ChunkEmbedding, ChunkMatch};
