# AI Engine — Sub-project A: Providers + RAG Pipeline

**Date:** 2026-04-13
**Status:** Approved
**Scope:** New nexus-ai crate with provider traits, HTTP implementations, content chunking, vector storage, RAG query pipeline, and CLI commands
**Source:** PRD 12 (partial), Growth Plan Phase 3

---

## Overview

New `nexus-ai` crate delivering the core intelligence layer: provider-agnostic AI chat and embedding traits with Anthropic/OpenAI/Ollama implementations, block-level content chunking, SQLite-backed vector storage with brute-force cosine similarity, and a RAG pipeline that answers questions using forge content as context. Exposed via CLI commands replacing the existing `ai` stub.

Sub-project B (streaming, tool use, inline assist, context assembly, conversation persistence, routing, rate limiting, privacy) is deferred until consumers exist.

---

## 1. Crate Structure

```
crates/nexus-ai/
├── Cargo.toml          (depends on reqwest, async-trait, serde, serde_json, rusqlite, tokio)
├── src/
│   ├── lib.rs          — public API facade
│   ├── error.rs        — AiError type
│   ├── config.rs       — AiConfig, provider selection from env vars
│   ├── provider.rs     — AiProvider trait, ChatMessage, Role
│   ├── embedding.rs    — EmbeddingProvider trait
│   ├── anthropic.rs    — Anthropic Claude implementation
│   ├── openai.rs       — OpenAI chat + embedding implementation
│   ├── ollama.rs       — Ollama chat + embedding implementation
│   ├── chunker.rs      — Block-level content chunking
│   ├── vectorstore.rs  — SQLite-backed vector store (BLOB embeddings, brute-force cosine)
│   └── rag.rs          — RAG pipeline orchestrator
```

Added to workspace `Cargo.toml` members list.

## 2. Provider Traits

### AiProvider (chat)

```rust
#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn chat(&self, messages: &[ChatMessage], system: Option<&str>) -> Result<String, AiError>;
    fn model_name(&self) -> &str;
}

pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

pub enum Role { System, User, Assistant }
```

### EmbeddingProvider

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AiError>;
    fn dimension(&self) -> usize;
}
```

### Implementations

All use `reqwest::Client` for HTTP.

| Provider | Chat Model Default | Embedding Model | Embedding Dims |
|---|---|---|---|
| Anthropic | claude-sonnet-4-20250514 | N/A (use OpenAI/Ollama for embeddings) | — |
| OpenAI | gpt-4o | text-embedding-3-small | 1536 |
| Ollama | llama3.2 | nomic-embed-text | 768 |

### Configuration

```rust
pub struct AiConfig {
    pub provider: String,       // "anthropic" | "openai" | "ollama"
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: u32,        // default: 4096
}
```

Provider selection from environment:
- `ANTHROPIC_API_KEY` set → Anthropic provider
- `OPENAI_API_KEY` set → OpenAI provider
- `OLLAMA_BASE_URL` set (or default `http://localhost:11434`) → Ollama provider

Embedding provider selected independently: prefers OpenAI if key available, falls back to Ollama.

## 3. Content Chunker

Uses existing parsed blocks as natural chunk boundaries.

```rust
pub struct Chunk {
    pub file_path: String,
    pub block_id: u64,
    pub content: String,
    pub heading_context: String,
}
```

`chunks_from_blocks(file_path: &str, blocks: &[BlockRecord], max_chunk_size: usize) -> Vec<Chunk>`

- Each block becomes one chunk
- Blocks exceeding `max_chunk_size` (default 2000 chars) split on sentence boundaries
- Each chunk prepended with nearest heading as context: `"## Section Name\n\n{block content}"`

## 4. Vector Store

Embeddings stored in `.forge/index.db` via schema migration v4.

### Schema

```sql
CREATE TABLE embeddings (
    id          INTEGER PRIMARY KEY,
    file_path   TEXT NOT NULL,
    block_id    INTEGER NOT NULL,
    chunk_text  TEXT NOT NULL,
    embedding   BLOB NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_embeddings_file ON embeddings(file_path);
```

Embeddings stored as raw BLOB — `Vec<f32>` serialized as little-endian bytes.

### VectorStore API

```rust
pub fn upsert(conn: &Connection, file_path: &str, chunks: &[ChunkEmbedding]) -> Result<()>
pub fn delete_by_file(conn: &Connection, file_path: &str) -> Result<()>
pub fn search(conn: &Connection, query_embedding: &[f32], limit: usize) -> Result<Vec<ChunkMatch>>
pub fn count(conn: &Connection) -> Result<usize>
```

`search` loads all embeddings into memory, computes cosine similarity in Rust, returns top-K. Sufficient for <10K chunks. Can be upgraded to sqlite-vec later.

### Types

```rust
pub struct ChunkEmbedding {
    pub file_path: String,
    pub block_id: u64,
    pub chunk_text: String,
    pub embedding: Vec<f32>,
}

pub struct ChunkMatch {
    pub file_path: String,
    pub block_id: u64,
    pub chunk_text: String,
    pub score: f32,
}
```

## 5. RAG Engine

```rust
pub struct RagEngine {
    ai_provider: Box<dyn AiProvider>,
    embedding_provider: Box<dyn EmbeddingProvider>,
}
```

### Query Flow

`query(conn, question, limit) -> Result<RagResponse>`:
1. Embed the question
2. Search vector store for top-K similar chunks
3. Build system prompt with retrieved chunks as context (with `[[wikilink]]` citations)
4. Send to AI provider with user's question
5. Return answer + source references

```rust
pub struct RagResponse {
    pub answer: String,
    pub sources: Vec<ChunkMatch>,
    pub model: String,
}
```

### Indexing

`index_file(conn, embedding_provider, file_path, blocks) -> Result<usize>` — chunk, embed, upsert. Returns chunk count.

`index_all(conn, embedding_provider, storage_conn) -> Result<usize>` — iterate all files, chunk and embed each.

## 6. CLI Commands

Replace existing `ai` stub with:

```
nexus ai ask <question>           — RAG query against the forge
nexus ai embed                    — rebuild all embeddings
nexus ai embed --file <path>      — embed a single file
nexus ai status                   — show embedding stats
nexus ai config                   — show active provider configuration
```

## 7. Testing

- **Chunker**: single block, oversized split, heading context
- **Vector store**: upsert + search round-trip (in-memory SQLite), cosine similarity correctness, delete_by_file, count
- **Provider request/response**: test serialization/deserialization of API payloads without live HTTP
- **RAG**: insert known embeddings, search, verify correct chunks returned
- **Schema**: migration v4 creates embeddings table

No live API tests in CI. Optional end-to-end test gated on API key env var.

## 8. Dependencies

New workspace dependencies:
```toml
reqwest = { version = "0.12", features = ["json"] }
```

The `nexus-ai` crate depends on: `reqwest`, `async-trait`, `serde`, `serde_json`, `rusqlite`, `tokio`, `thiserror`.

The schema migration v4 for the embeddings table lives in `nexus-storage/src/schema.rs`.

## 9. Files Changed/Created

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add nexus-ai to members, add reqwest |
| `crates/nexus-ai/Cargo.toml` | **NEW** |
| `crates/nexus-ai/src/*.rs` | **NEW** — 11 source files |
| `crates/nexus-storage/src/schema.rs` | Add migration v4 (embeddings table) |
| `crates/nexus-cli/Cargo.toml` | Add nexus-ai dependency |
| `crates/nexus-cli/src/main.rs` | Replace Ai stub with real command group |
| `crates/nexus-cli/src/commands/ai.rs` | **NEW** — AI command handlers |
| `crates/nexus-cli/src/commands/mod.rs` | Register ai module |

## Out of Scope

- Streaming responses
- Tool use / function calling
- Inline assist (ghost text)
- Context assembly engine
- Conversation persistence
- Provider routing / fallback / load balancing
- Rate limiting
- Privacy controls
- Event-driven embedding updates (deferred — manual `nexus ai embed` for now)
