//! Nexus AI engine: provider traits, embeddings, and RAG pipeline.
//!
//! This crate defines the core abstractions for interacting with AI
//! language models and embedding services, along with concrete
//! implementations for Anthropic, `OpenAI`, and Ollama.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod anthropic;
mod chunker;
mod config;
mod embedding;
mod error;
mod ollama;
mod openai;
mod provider;
mod rag;
mod vectorstore;
pub mod core_plugin;

pub use core_plugin::AiCorePlugin;
pub use anthropic::AnthropicProvider;
pub use chunker::{chunks_from_blocks, Chunk};
pub use config::{detect_embedding_provider, detect_provider, AiConfig};
pub use embedding::EmbeddingProvider;
pub use error::AiError;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use provider::{AiProvider, ChatMessage, Role};
pub use rag::{index_file as rag_index_file, query as rag_query, RagResponse};
pub use vectorstore::{
    count as vectorstore_count, delete_by_file as vectorstore_delete,
    search as vectorstore_search, upsert as vectorstore_upsert, ChunkEmbedding, ChunkMatch,
};
