# com.nexus.ai

- **Path:** `crates/nexus-ai/`
- **Tier:** Core Rust
- **Bootstrap order:** 8

## Architecture

- Entry point: `crates/nexus-ai/src/core_plugin.rs` — `AiCorePlugin` implements `nexus_plugins::CorePlugin`. Bootstrap registration: `crates/nexus-bootstrap/src/plugins/ai.rs`. Lifecycle: `on_init` (detect providers from env) + `on_stop` (tear down indexing daemon). `wire_context` spawns the BL-041 background indexing daemon.
- Key modules: `provider.rs` (`AiProvider` trait + `ChatTurn` types), `anthropic.rs` / `openai.rs` / `ollama.rs` (provider impls), `embedding.rs` + `local_embedding.rs` (fastembed-rs behind `local-embeddings` feature, ADR 0018), `rag.rs` + `chunker.rs` + `vectorstore.rs` (RAG pipeline; vector storage routes through `com.nexus.storage` IPC — this crate never touches SQLite directly), `tools/` (`ToolRegistry`, MCP bridge, dispatch target), `enrichment.rs`, `indexing_daemon.rs`, `sanitize.rs` (prompt-injection scanner), `privacy.rs` (redaction), `tokens.rs` (budget counter), `activity_log.rs`, `handlers/` (one file per handler family), `generate_docs.rs`, `http_client.rs` (TLS-pinned via `nexus-security`).
- Persistence: append-only `.forge/ai-activity.log` via `ActivityRecorder` (`activity_log.rs:42`). Chat sessions and the vector store live in the storage plugin's SQLite database — this crate carries no DB of its own.
- Settings owned: `<forge>/.forge/ai.toml` — `AiConfig` (provider, model, embedding model, temperature, predict-token budget, etc.). See `docs/0.1.2/settings/forge-config.md` (table row at `forge-config.md:28`).
- External dependencies: `reqwest` (network egress to provider HTTPS), optional `fastembed` + ONNX Runtime (libonnxruntime via `ort-load-dynamic` — runtime dynlib lookup). TLS pinning enforced by `nexus-security`.

## Surface

25 IPC handlers (full table at `crates/nexus-ai/src/core_plugin.rs:240`):

`ask`, `index_file`, `vectorstore_count`, `status`, `config`, `stream_chat`, `stream_ask`, `session_load`, `session_save`, `session_list`, `session_delete`, `set_config`, `semantic_search`, `index_status`, `enrich_file`, `enrich_apply`, `index_trigger`, `activity_list`, `activity_clear`, `propose_tool_calls`, `resolve_credentials`, `generate_docs`, `entity_recall`, `enrich_entity`, `infer_entity_relations`.

Bus topics: `com.nexus.ai.stream_chunk`, `com.nexus.ai.stream_start`, `com.nexus.ai.stream_done`, plus `ACTIVITY_APPENDED_TOPIC` / `AI_ACTIVITY_APPENDED_TOPIC` envelopes.

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No — the basic-capability workflow (open forge, browse, edit, search, commit) involves no AI calls. Chat / ask / semantic search / inline enrichment are all features layered on top.
- **Depended on by:** `com.nexus.agent` (planning + tool loop go through `com.nexus.ai`), `com.nexus.ai.runtime` (republishes `stream_*` topics; also imports `nexus-ai-runtime` itself — cycle is one-way in the runtime direction), `nexus-ai`'s indexing daemon is consumed by shell `semanticSearch`, MCP server, and any community plugin holding the relevant capability tokens.
- **Depends on:** `com.nexus.storage` (vector + session IPC), `com.nexus.security` (HTTPS pinning), `com.nexus.ai.runtime` (shared tokio pool handle).
- **What breaks if removed:** every AI-driven feature — chat, ask, ghost-text predict, inline enrichment, entity inference, generate-docs, semantic search, RAG. The agent and ai-runtime plugins lose their planning backend. The minimum-viable workflow is intact.

## Notes

- A `HANDLER_PREDICT = 26` constant exists at `core_plugin.rs:233` and is dispatched in `dispatch_async` (`core_plugin.rs:475`), but it is **not** listed in the `IPC_HANDLERS` table. That means the bootstrap manifest does not advertise `predict` as a callable command — only direct internal callers reach it. Likely audit follow-up.
- `local-embeddings` feature is off by default; minimal builds skip `fastembed` and the ONNX Runtime lookup.
- `wire_context` also seeds the `ToolRegistry` with storage-backed `read_file` / `write_file` built-ins (`tools/functions.rs`) and the MCP bridge so any provider supporting tool-calling can route through kernel IPC.
- `nexus-ai-runtime` is a hard dep (line `Cargo.toml:19`) — the indexing daemon piggybacks on the runtime's shared tokio handle to avoid building a second runtime.
