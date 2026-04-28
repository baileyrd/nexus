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

mod anthropic;
mod chunker;
mod config;
pub mod core_plugin;
mod embedding;
mod error;
mod ollama;
mod openai;
pub mod privacy;
mod provider;
mod rag;
mod tokens;
mod vectorstore;

pub use anthropic::AnthropicProvider;
pub use chunker::{chunks_from_blocks, Chunk};
pub use config::{detect_embedding_provider, detect_provider, AiConfig};
pub use core_plugin::AiCorePlugin;
pub use embedding::EmbeddingProvider;
pub use error::AiError;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use privacy::{PrivacyPolicy, Redaction, Redactor};
pub use provider::{AiProvider, ChatMessage, Role};
pub use rag::{index_file as rag_index_file, query as rag_query, RagResponse};
pub use tokens::{ApproxTokenCounter, BudgetWarning, ContextSourceKind, TokenBudget, TokenCounter};
pub use vectorstore::{ChunkEmbedding, ChunkMatch};
