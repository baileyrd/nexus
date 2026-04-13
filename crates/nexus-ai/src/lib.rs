//! Nexus AI engine: provider traits, embeddings, and RAG pipeline.
//!
//! This crate defines the core abstractions for interacting with AI
//! language models and embedding services, along with concrete
//! implementations for Anthropic, OpenAI, and Ollama.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod anthropic;
mod config;
mod embedding;
mod error;
mod ollama;
mod openai;
mod provider;

pub use anthropic::AnthropicProvider;
pub use config::{detect_embedding_provider, detect_provider, AiConfig};
pub use embedding::EmbeddingProvider;
pub use error::AiError;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use provider::{AiProvider, ChatMessage, Role};
