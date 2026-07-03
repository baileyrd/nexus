# nexus-ai

> Kind: lib · IPC plugin id: com.nexus.ai · CorePlugin: yes · Has settings: AiConfig · As of: 2026-05-25

## Overview

`nexus-ai` is the AI engine. It owns the provider abstraction (chat + embeddings), the retrieval-augmented-generation (RAG) pipeline, the tool-calling / function-calling machinery, the background re-indexing daemon, and a suite of higher-level AI verbs (note enrichment, entity recall/enrichment, doc generation, per-keystroke code prediction). It registers as a single `CorePlugin` under id `com.nexus.ai` and exposes everything to the rest of the runtime as async IPC handlers — CLI, TUI, MCP, the Tauri shell, `nexus-agent`, `nexus-workflow`, and `nexus-audio` all reach it through `context.ipc_call("com.nexus.ai", …)`. Per the file-as-truth invariant the crate never opens SQLite; all vector storage goes out as nested `ipc_call`s to `com.nexus.storage`'s `vector_*` / `query_*` handlers (see [`vectorstore.rs`](../../../crates/nexus-ai/src/vectorstore.rs)).

The provider layer is a pair of object-safe traits — [`AiProvider`](../../../crates/nexus-ai/src/provider.rs) (chat / streaming / tool-aware chat) and [`EmbeddingProvider`](../../../crates/nexus-ai/src/embedding.rs) — with three remote implementations: `AnthropicProvider`, `OpenAiProvider` (which also implements `EmbeddingProvider`), and `OllamaProvider` (chat + embeddings + a `/api/generate` FIM path). A fourth, local, embedding backend `LocalEmbedding` (fastembed-rs / ONNX, BL-019, ADR 0018) is compiled only under the optional `local-embeddings` Cargo feature, which also pulls in a `DashMap` + `xxhash3` embedding cache. Outbound provider HTTPS is built through `nexus_security::tls::build_pinned_client` ([`http_client.rs`](../../../crates/nexus-ai/src/http_client.rs)), so TLS pinning (BL-102) is one flag away on the Anthropic / OpenAI clients.

The crate builds on `nexus-ai-runtime` only for a shared tokio worker-pool handle (read-only, via the `shared_pool_handle()` free function) used by the BL-041 background indexing daemon — there is no IPC reach back into the runtime. Microkernel isolation holds: `nexus-ai` is a subsystem crate that depends on `nexus-kernel` / `nexus-plugins` / `nexus-types` / `nexus-security`; the kernel never depends on it.

Capability gating is layered. The plugin's own `KernelPluginContext` holds `IpcCall` (and FS caps for session-file persistence) and every nested storage / git / terminal / mcp call passes through the target plugin's capability check. Caller-facing gates are declared in `nexus-bootstrap`'s `cap_matrix.toml`: chat-class verbs require `ai.chat`, with an args-aware policy (`extra_caps_for_policy`) that adds `ai.tools.write` for `tools=auto` and `ai.tools.mcp` for `auto_with_mcp`. Two cross-cutting safety passes run on retrieved content before it reaches a model: an outbound secret redactor ([`privacy.rs`](../../../crates/nexus-ai/src/privacy.rs), BL-017) and an inbound prompt-injection scanner ([`sanitize.rs`](../../../crates/nexus-ai/src/sanitize.rs), BL-130), both wired into the RAG prompt builder.

## Position in the dependency graph

**Direct `nexus-*` dependencies** (from `Cargo.toml`):
- `nexus-kernel` — `KernelPluginContext`, event bus, IPC traits (`Ipc`, `Events`, `FileSystem`, `Identity`), `Capability`.
- `nexus-plugins` — `CorePlugin` / `CorePluginFuture` / `PluginError`, `define_dispatch_helpers!`.
- `nexus-types` — BL-052 shared activity-timeline types (`ActivityEntry`, `ActivitySurface`, `ActivityOutcome`, `ActivityToolCall`, topics) so non-AI emitters publish without depending on this crate.
- `nexus-security` — BL-102 TLS-pinning verifier (`tls::build_pinned_client`, `tls_pins::HOST_PINS`).
- `nexus-ai-runtime` — BL-134 Phase 4 shared tokio pool handle (read-only free function; no IPC reach).

**Notable external dependencies:** `reqwest` (provider HTTP), `async-trait`, `futures` (Ollama NDJSON byte-stream), `regex-lite` (privacy + sanitize patterns), `sha2` (enrichment body hash), `uuid`, `chrono`. Feature-gated: `fastembed` v5 (`ort-load-dynamic`, `hf-hub-rustls-tls`), `dashmap` v6, `xxhash-rust` (`xxh3`) — all only under `local-embeddings`. Optional `ts-rs` + `schemars` under `ts-export` for IPC binding generation.

**Crates depending on it:** `nexus-bootstrap` (registers the plugin, threads cap policies, wraps `stream_chat` as `AiChatDriver` for the agent). `nexus-agent` references `nexus_ai::ipc` types for plan derivation (`propose_tool_calls` reply → `Plan`). The crate is registered at bootstrap by [`crates/nexus-bootstrap/src/plugins/ai.rs`](../../../crates/nexus-bootstrap/src/plugins/ai.rs).

## Public API surface

### Provider layer
- `provider::AiProvider` (trait) — `chat(messages, system)`, `chat_stream_with(…, on_chunk)` (default = collect-then-emit), `chat_turn_with_tools(turns, system, tools, on_chunk)` (default = lossy fallback to `chat_stream_with`), `model_name()`.
- `provider::{ChatMessage, Role, ChatTurn, ChatTurnOutput, ToolCall}` — chat wire types; `ChatTurn` is the rich tool-aware turn (User / Assistant{content, tool_calls} / ToolResult{tool_use_id, content, is_error}).
- `provider::turns_to_legacy_messages` — lossy `ChatTurn` → `ChatMessage` projection for the trait default.
- `embedding::EmbeddingProvider` (trait) — `embed(texts) -> Vec<Vec<f32>>`, `dimension()`.
- `anthropic::AnthropicProvider` — Anthropic Messages API (non-streaming; `chat_turn_with_tools` parses `tool_use` blocks, tolerates unknown block types like `thinking`). `DEFAULT_MODEL = "claude-sonnet-4-20250514"`.
- `openai::OpenAiProvider` — OpenAI chat + embeddings. Implements both traits. Tool args round-trip as JSON-encoded strings. `DEFAULT_CHAT_MODEL = "gpt-4o"`, `DEFAULT_EMBEDDING_MODEL = "text-embedding-3-small"` (1536-dim).
- `ollama::OllamaProvider` — local/remote Ollama. True streaming via NDJSON `bytes_stream`; `chat_turn_with_tools` (tool args are JSON objects, synthesises missing tool-call ids); `fim_generate` (`/api/generate` with `suffix`, retries without suffix on `400 … does not support insert`). `DEFAULT_BASE_URL = http://localhost:11434`, `DEFAULT_CHAT_MODEL = "llama3.2"`, `DEFAULT_EMBEDDING_MODEL = "nomic-embed-text"` (768-dim), `DEFAULT_FIM_TEMPERATURE = 0.2`. `think: false` baked into requests to suppress Qwen3 empty-content responses.
- `local_embedding::LocalEmbedding` *(feature `local-embeddings`)* — fastembed-backed `EmbeddingProvider`; `new`/`with_capacity`, `embed_batch` (cache-bypass over `BATCH_CACHE_BYPASS_THRESHOLD = 1000`), `cache_len`. `map_model` aliases → fastembed `EmbeddingModel`; `dimension_for` resolves dims without loading weights. `DEFAULT_LOCAL_MODEL = "bge-small-en-v1.5-int8"` (384-dim), `DEFAULT_CACHE_MAX_ENTRIES = 50_000`.

### RAG / vector store
- `rag::query` — full RAG: embed → `vectorstore::search` → redact+scan chunks → build grounded system prompt → `chat` → build citations. Returns `RagResponse{answer, sources, citations, model}`.
- `rag::{retrieve, semantic_search}` — retrieval half (embed + search, no chat). `rag::index_file` — chunk → embed → `vectorstore::upsert` (or `delete_by_file` when empty).
- `rag::{build_rag_prompt, build_rag_prompt_budgeted, build_rag_prompt_budgeted_with_scanner}` — prompt assembly under a `TokenBudget`, with optional `Redactor` (outbound) + `Scanner` (inbound injection). `build_citations` enriches sources with line ranges via `query_blocks` and renumbers by `[N]` markers.
- `rag::{Citation, RagResponse}` — public RAG result types.
- `vectorstore::{ChunkEmbedding, ChunkMatch}` + free fns `upsert / search / delete_by_file / count` (all storage-IPC clients). Bounds: `MAX_CHUNKS_PER_UPSERT = 4096`, `MAX_CHUNK_TEXT_BYTES = 256 KiB`, `MAX_EMBEDDING_DIM = 12_288`.
- `chunker::{Chunk, chunks_from_blocks}` — splits markdown blocks into embeddable chunks, prepending the current heading; `DEFAULT_MAX_CHUNK_SIZE = 1024`.

### Tools / function-calling (`tools` module)
- `tools::{ToolRegistry, ToolExecutor, ToolSchema, RegisteredTool, ToolError}` — in-process registry (BL-016). `ToolError` = `NotFound(name)` / `ExecutionFailed` / `InvalidInput`.
- Built-ins (all route through `ipc_call`): `ReadFileTool`/`WriteFileTool` (`register_storage_builtins`), `SearchForgeTool`/`ListBacklinksTool`/`GitLogTool` (`register_extended_builtins`), `TerminalRunSavedTool`/`TerminalGetStatusTool`/`TerminalSendSignalTool` (`register_terminal_builtins`, BL-055).
- `tools::{dispatch_target, DispatchTarget, DispatchTargetError}` — map a tool name → `(target_plugin_id, command_id, args)` triple (reshapes `write_file` `content`→`bytes`; parses `mcp__<server>__<tool>`).
- `tools::{discover_mcp_tools, McpToolExecutor}` — MCP bridge for `tools=auto_with_mcp`.

### Config / detection
- `config::AiConfig` + `detect_provider` / `detect_embedding_provider` / `detect_local_embedding` (env-var auto-detection).

### Safety / budget
- `privacy::{PrivacyPolicy, Redaction, Redactor}` — outbound secret redaction (6 high-confidence patterns).
- `sanitize::{Scanner, ScanResult, Finding, InjectionPolicy, InjectionSource}` — inbound injection detection (4 pattern families, 4 policies).
- `tokens::{TokenCounter, ApproxTokenCounter, TokenBudget, BudgetWarning, ContextSourceKind}` — ~4-chars/token budget arithmetic; no tokenizer dependency.

### Plugin / activity / enrichment
- `core_plugin::{AiCorePlugin, PLUGIN_ID, IPC_HANDLERS, MANIFEST_DEPS, HANDLER_* consts}`.
- `activity_log::{ActivityRecorder, ACTIVITY_LOG_PATH}` + re-exported `nexus_types::activity::*`.
- `enrichment::{EnrichmentProposal, propose, apply, body_hash, merge_frontmatter, strip_frontmatter}` (module is `pub`, free fns `propose`/`apply` are crate-internal).
- `indexing_daemon` (`pub` module) — `IndexingDaemon`, `IndexStatus`, `DaemonMsg`, `SharedStatus`, `EmbedderFactory`, `DEFAULT_DEBOUNCE = 2s`.
- `ipc` module — typed arg/return structs (see IPC handlers). `error::AiError`.

## IPC handlers

Handler ids and `(command, id)` pairs come from `core_plugin::IPC_HANDLERS` / `HANDLER_*` constants. `config`, `resolve_credentials`, and `index_status` resolve synchronously (also mirrored on the async path); everything else is async. Capabilities below are the caller-facing gates from `nexus-bootstrap/cap_matrix.toml` (the plugin's own context only ever needs `IpcCall`).

| command | id | args | returns | capability | description |
|---|---|---|---|---|---|
| `ask` | 1 | `{question: String, limit?: usize=5}` | `RagResponse` JSON | `ai.chat` | One-shot RAG: embed → vector search → grounded chat. Threads `injection_policy`. |
| `index_file` | 2 | `{file_path: String, blocks: [(id,kind,text,pos)]}` | `{indexed_chunks: usize}` | `ai.index` | Chunk + embed + upsert vectors via storage. |
| `vectorstore_count` | 3 | none | `{count: usize}` | unrestricted (read-only) | Proxy to `storage::vectorstore_count`. |
| `status` | 4 | none | `{ai_provider, ai_model, embedding_provider, embedding_model, embedding_dimension, indexed_chunks, tls_pinned, local_embeddings_supported}` | unrestricted (read-only) | Provider + index summary. |
| `config` | 5 | none | `{ai: ConfigView?, embedding: ConfigView?}` (no secrets) | unrestricted (read-only) | Detected/active provider snapshot. Sync. |
| `stream_chat` | 6 | `AiStreamChatArgs` (messages, system?, session_id?, mode?, tools?, max_tokens?, stop?, trim?, surface?) | `{session_id, text}` | `ai.chat` (+`ai.tools.write` if `tools=auto`, +`ai.tools.mcp` if `auto_with_mcp`) | Direct chat; per-token `stream_*` bus events. `mode=chat` runs the tool-dispatch loop; `mode=complete` is a single round-trip, no tools, post-processed (ghost completion / headless `complete`). |
| `stream_ask` | 7 | `AiStreamAskArgs` (messages, session_id?, limit?=5) | `AiStreamAskResult{session_id, text, sources}` (+citations on event) | `ai.chat` | RAG retrieve + streaming chat with citations. |
| `session_load` | 8 | `{id?}` | persisted session JSON or `null` | `ai.session.read` | Read `<forge>/.forge/chat/sessions/<id>.json` (or legacy `chat_session.json`). |
| `session_save` | 9 | opaque `{id?, …}` object | `{bytes, id}` | `ai.session.write` | Overwrite persisted session JSON. |
| `session_list` | 10 | none | `[{id, title?, updated_at, bytes}]` | `ai.session.read` | Enumerate multi-session files. |
| `session_delete` | 11 | `{id: String}` | `{deleted: true, id}` | `ai.session.write` | Remove a multi-session file (id validated). |
| `set_config` | 12 | `{ai?: {provider, model?, api_key?, base_url?, …} \| null, embedding?: … \| null}` | `config` snapshot | `ai.config.write` | Hot-swap in-memory `AiConfig` (chat/embedding) at runtime; null clears, absent leaves untouched. |
| `semantic_search` | 13 | `{query: String, limit?: usize=10}` | `{matches: Vec<ChunkMatch>}` | `ai.chat` | BL-040 embed + top-N vector hits, no chat. |
| `index_status` | 14 | none | `IndexStatus{indexed_files, pending_files, total_seen, last_error, running}` | unrestricted (read-only) | BL-041 daemon counters. Sync. |
| `enrich_file` | 15 | `{path: String}` | `EnrichmentProposal` | `ai.chat` | BL-045 propose tags+summary+related (no write). |
| `enrich_apply` | 16 | `{proposal: EnrichmentProposal}` | `{applied: bool, reason?}` | unrestricted (storage gates downstream) | Merge proposal into YAML frontmatter iff `body_hash` matches. |
| `index_trigger` | 17 | none | `{queued: usize}` | `ai.index` | FU-2 fan all forge markdown (`storage::query_files`) into the daemon as `Touched`. |
| `activity_list` | 18 | `AiActivityListArgs{limit?}` | `AiActivityListResult{entries}` (newest-first) | unrestricted (read-only) | BL-037 read AI activity timeline. |
| `activity_clear` | 19 | none | `{cleared: bool}` | `ai.activity.write` | Truncate activity log. |
| `propose_tool_calls` | 20 | `AiProposeArgs{messages, system?, tools?}` | `AiProposeReply{text, tool_calls, unmapped_tool_calls}` | `ai.chat` (+policy caps) | G7 single-turn provider call returning mapped tool-use blocks WITHOUT executing (used by `nexus-agent` to derive a Plan). No system-prompt floor. |
| `resolve_credentials` | 21 | none | `{provider, api_key, base_url, model} \| null` | internal (Core-trust callers only) | BL-117 return live chat provider creds for sibling subsystems (e.g. `nexus-audio`). Sensitive. Sync. |
| `generate_docs` | 22 | `AiGenerateDocsArgs{symbol_id? \| path?+name?, style?}` | `AiGenerateDocsReply{docblock, symbol_id, language, kind, name, path, insert_line, degraded, degraded_reason?}` | `ai.chat` | BL-116 generate a language-appropriate docblock from the BL-114 symbol index. No write-back. |
| `entity_recall` | 23 | `EntityRecallArgs{query, limit?=5}` | `EntityRecallResult{results: [EntityRecallHitRow]}` | unrestricted (read-only) | BL-128 FAISS-backed recall over `entities/` corpus (oversample → group by file → resolve via `entity_get`). |
| `enrich_entity` | 24 | `EnrichEntityArgs{entity_id, min_description_chars?=80, dry_run?}` | `EnrichEntityResult{entity_id, original_description, new_description, skipped, applied}` | `ai.chat` | BL-129 expand an entity description (writes via `entity_upsert` unless dry_run). |
| `infer_entity_relations` | 25 | `InferEntityRelationsArgs{entity_id, max_proposals?=3, dry_run?}` | `InferEntityRelationsResult{entity_id, proposals, applied}` | `ai.chat` | BL-129 propose new entity relations at `confidence: 0.5` (writes via `entity_upsert` unless dry_run). |
| `predict` | 26 | `AiPredictArgs{prefix, suffix, language, file_path, max_tokens?}` | `AiPredictReply{completion}` | `ai.chat` | BL-139 per-keystroke FIM. Ollama `/api/generate` (suffix); chat-shaped FIM fallback for OpenAI/Anthropic. Not listed in `IPC_HANDLERS` (reached via streaming surfaces). |
| `extract_entities` | 30 | `ExtractEntitiesArgs{path, max_entities?=3, dry_run?}` | `ExtractEntitiesResult{path, created, proposals}` | `ai.chat` | C44 (#397) read a note via `storage::read_file`, ask the provider to name distinct entities it substantively discusses, create each genuinely-new one as a bare entity stub via `entity_upsert` (no relations — `infer_entity_relations` picks it up next cycle); entities that already exist are always skipped, never re-enriched. The BL-129 Dream Cycle's `extract` phase (opt-in, `[dream_cycle].extract_enabled`) drives this over recently-changed notes. |

Note: `nexus-bootstrap` registers v1 aliases (`with_v1_aliases`) for these commands. `HANDLER_PREDICT` (26) is intentionally absent from `IPC_HANDLERS` but is dispatched in `dispatch_async`.

## Capabilities

The `com.nexus.ai` plugin context (granted at bootstrap) holds `IpcCall` plus the FS caps needed for session-file persistence; every nested storage/git/terminal/mcp call is re-checked against the *target* plugin's gate. The built-in tools deliberately route through `ipc_call` rather than depending on `nexus-storage` / `nexus-editor` directly (CLAUDE.md invariant 3).

Caller-facing gates (declared in `cap_matrix.toml`, ADR 0022):
- `ai.chat` — all chat-class verbs (`ask`, `stream_chat`, `stream_ask`, `semantic_search`, `enrich_file`, `enrich_entity`, `infer_entity_relations`, `extract_entities`, `propose_tool_calls`, `generate_docs`, `predict`).
- Args-aware policy `ai_tools_policy` (`ipc::extra_caps_for_policy`): `tools=auto` ⇒ +`ai.tools.write`; `auto_with_mcp` ⇒ +`ai.tools.write` +`ai.tools.mcp`; `none` / `auto_readonly` ⇒ no extra caps. Applies to `stream_chat` + `propose_tool_calls`.
- `ai.index` (`index_file`, `index_trigger`), `ai.session.read` / `ai.session.write`, `ai.config.write` (`set_config` — flagged "equivalent in surface to process.spawn"), `ai.activity.write` (`activity_clear`).
- Read-only/unrestricted: `status`, `config`, `index_status`, `vectorstore_count`, `activity_list`, `entity_recall`, `enrich_apply`.
- `resolve_credentials` is `internal = true` — reachable only from a Core-trust caller (audio/agent) regardless of cap set; returns provider keyring material.

**Net access / TLS pinning (BL-102):** Anthropic/OpenAI clients are built via `nexus_security::tls::build_pinned_client(tls_pinning_enabled)`. Pinning is effective iff `AiConfig::tls_pinning_enabled` (sourced from `KernelConfig::tls_pinning_enabled`) OR `NEXUS_TLS_PINNING=1`. When on, every handshake's leaf cert SHA-256 must match `nexus_security::tls_pins::HOST_PINS` (shipped empty; operator seeds it). The Ollama client is plain `reqwest::Client::new()` (local endpoint). The `status` handler reports the effective pinning state via `tls_pinning_effective`.

## Settings / Config

`AiConfig` ([`config.rs`](../../../crates/nexus-ai/src/config.rs)) is the in-memory config struct (not serde-derived itself — it is populated by env detection at `on_init` and by the `set_config` parser). Persistence lives in the shell's config store / `ai.toml`; the shell pushes the persisted config via `set_config` on every boot, so env detection is the floor.

| field | default | source / notes |
|---|---|---|
| `provider` | `""` | `"anthropic"`/`"openai"`/`"ollama"`/`"local"`. From `ANTHROPIC_API_KEY` → `OPENAI_API_KEY` → `OLLAMA_BASE_URL` detection order. |
| `model` | `None` | per-request model override. |
| `api_key` | `None` | from env or `set_config`. |
| `base_url` | `None` | self-hosted / proxy / Ollama URL. |
| `max_tokens` | `4096` | generation cap. |
| `context_window` | `8192` | total window for `TokenBudget`. |
| `reserved_response_tokens` | `1024` | reserved out of `context_window`. |
| `privacy` | `PrivacyPolicy::Off` | outbound redaction policy. |
| `injection_policy` | `InjectionPolicy::Off` | BL-130 inbound scanner policy for RAG chunks. |
| `local_embedding_model` | `None` | read only when `provider == "local"`; default `bge-small-en-v1.5-int8`. |
| `tls_pinning_enabled` | `false` | BL-102; from `KernelConfig::tls_pinning_enabled`. |
| `predict_max_tokens` | `64` | BL-139 FIM cap when caller omits `max_tokens`; `ai.toml [ai] predict_max_tokens`. |
| `anthropic_model` | `None` | P2-04 per-provider default; falls back to `DEFAULT_MODEL`. |
| `openai_chat_model` | `None` | P2-04; falls back to `DEFAULT_CHAT_MODEL`. |
| `openai_embedding_model` | `None` | P2-04; falls back to `DEFAULT_EMBEDDING_MODEL`. |
| `ollama_chat_model` | `None` | P2-04; falls back to `DEFAULT_CHAT_MODEL`. |
| `ollama_embedding_model` | `None` | P2-04; falls back to `DEFAULT_EMBEDDING_MODEL`. |
| `ollama_temperature` | `None` | P2-04; FIM `/api/generate` temperature (default `0.2`). |
| `indexing_debounce_secs` | `None` | P2-06 daemon debounce; `None` ⇒ `DEFAULT_DEBOUNCE` (2s). `ai.toml [ai] indexing_debounce_secs`. |

Env auto-detection: chat = `ANTHROPIC_API_KEY` → `OPENAI_API_KEY` → `OLLAMA_BASE_URL`; embedding = `NEXUS_LOCAL_EMBEDDINGS` (truthy) → `OPENAI_API_KEY` → `OLLAMA_BASE_URL`. `NEXUS_LOCAL_EMBEDDING_MODEL` overrides the local model id. API keys flow in-memory only; the plugin holds `ai_config` / `embed_config` in `Arc<RwLock<Option<AiConfig>>>` so `set_config` updates land without a restart.

The system-prompt floor `HOST_SYSTEM_PROMPT_FLOOR` (G3, in `handlers/shared.rs`) is prepended to every `mode=chat` system prompt (forge-relative paths, prefer tools, minimal edits); skipped for `mode=complete` and for `propose_tool_calls`. Tool-loop cap `MAX_TOOL_ROUNDS = 8`.

## Events

Published on the kernel bus (no IPC return for the per-token deltas):
- `com.nexus.ai.stream_start` — `{session_id}` (and `{sources}` for `stream_ask`). From `EngineEnvelope::publish_start` / the `stream_ask` handler.
- `com.nexus.ai.stream_chunk` — `{session_id, chunk, index}`, one per token delta.
- `com.nexus.ai.stream_done` — `{session_id, text}` (plus `{sources, citations}` for `stream_ask`).
- Activity-timeline topics (BL-052, re-exported from `nexus_types::activity`): `ACTIVITY_APPENDED_TOPIC` (universal) and `AI_ACTIVITY_APPENDED_TOPIC` (legacy AI-only) — published by `ActivityRecorder::append`.

Subscribed: the BL-041 `IndexingDaemon` subscribes to `com.nexus.storage.file_*` events on the kernel `EventBus` (created/modified/deleted/renamed), debounces bursts, and re-indexes affected files via `query_blocks` + `rag::index_file` (deletions → `vector_delete_by_file`).

## Internals & notable implementation details

- **Provider dispatch** — `handlers/shared::build_ai_provider` / `build_embedding_provider` route `provider` strings to concrete impls, layering per-request `model` over P2-04 per-provider defaults over the built-in constant. `local` embedding routes to `build_local_embedding_provider`, which returns a clear "rebuild with `--features local-embeddings`" error when the feature is off.
- **Tool-dispatch loop** (`run_tool_dispatch_loop`) — builds `ChatTurn`s, calls `chat_turn_with_tools`, executes each requested tool through the registry, feeds `ToolResult` turns back, loops until no tool calls or `MAX_TOOL_ROUNDS`. Records `ActivityToolCall{name, ok}` and extracts touched `path`s for the timeline. `mode=complete` (`run_complete`) physically bypasses this loop (single `chat_stream_with`), then optionally strips prompt-echo (`strip_prompt_echo`), clips at a natural break (`trim_to_natural_break`), and applies stop sequences (`apply_stop`).
- **Streaming** — Anthropic/OpenAI use non-streaming HTTP endpoints; their adapters emit each text block through `on_chunk` so the UI still sees deltas. Ollama does true NDJSON byte-stream parsing (`bytes_stream`, manual newline framing) and synthesises tool-call ids when older builds omit them. `EngineEnvelope` owns the `stream_start`/`stream_chunk`/`stream_done` framing so chat and complete paths emit byte-identical event streams.
- **Provider wire-format quirks** — OpenAI tool args are JSON-encoded *strings* (re-`from_str`'d into a `Value`); Ollama tool args are JSON *objects*. Anthropic batches consecutive `ToolResult`s into one `user` block-array message and keeps `system` in the top-level field; OpenAI/Ollama prepend `system` as a synthetic message and lack an `is_error` flag (errors are prefixed `[error] …`).
- **Embedding cache (local)** — `embed_with_cache` keys on `xxh3_64(text)`, interleaves hits/misses in input order, and does a coarse "clear all when over budget" eviction. Batches over 1000 entries skip the cache entirely. `fastembed::TextEmbedding::embed` takes `&mut self`, so it's wrapped in a `Mutex`.
- **RAG chunking/retrieval** — `chunks_from_blocks` prepends the current heading and splits at `DEFAULT_MAX_CHUNK_SIZE`. `build_rag_prompt_budgeted_with_scanner` sorts sources by descending score, redacts (outbound) then scans (inbound) each chunk before charging it to the budget, drops over-budget / `Reject`-policy chunks (recorded as `BudgetWarning`), and emits `NearLimit` at ≥80% utilisation. Citations attach line ranges via per-path `query_blocks` calls and renumber by first-occurrence of `[N]` markers in the answer.
- **Credential resolution / TLS pinning** — `build_client` re-exports `nexus_security::tls::build_pinned_client`; `resolve_credentials` returns the live chat provider's key from the `set_config`-mutated `RwLock` (gated internal; sensitive).
- **Indexing daemon** — `IndexingDaemon::start_with_debounce` spawns a worker thread that subscribes to storage file events, debounces (`DEFAULT_DEBOUNCE` / `DEFAULT_MAX_BATCH = 32`), and re-embeds via an `EmbedderFactory` closure that re-reads the live `embed_config` each batch (picks up `set_config` without restart). Status lives in `Arc<RwLock<IndexStatus>>`. Started in `wire_context` (first hook with the kernel context), joined in `on_stop`.
- **Activity log** — `ActivityRecorder` persists JSONL at `.forge/ai-activity.log`, head-truncated at `ACTIVITY_LOG_MAX_BYTES = 256 KiB`, serialised by an internal `Mutex`. Read fully into memory on each `activity_list` (no FTS in v1).
- **Session-id safety** — `validate_session_id` enforces `[A-Za-z0-9_-]{1,64}` and rejects path traversal; `session_path` routes legacy (no id) vs multi-session.

## Tests

Extensive in-module `#[cfg(test)]` coverage (no separate integration suite beyond one smoke test):
- `provider.rs` — `ChatTurn`→legacy projection, `ToolCall` serde round-trip.
- `anthropic.rs` / `openai.rs` / `ollama.rs` — tool-schema wire-shape, turn→provider-message translation (system handling, tool_use/tool_result batching, args-as-string vs args-as-object), response parsing (incl. Anthropic unknown-block tolerance, OpenAI invalid-args error), and Ollama streaming via a hand-rolled one-shot TCP server (synthesised ids, text-only, 500-error path). `ollama.rs` FIM-fallback substring detection.
- `core_plugin.rs` — AIG-05 local-embedding config round-trip (parse/snapshot/status/dimension, feature-gated), `AutoReadOnly` filter, system-prompt floor composition, session-id validation, and the full tool-dispatch loop (no-tool, execute+loop, unknown-tool error, max-rounds cap, `messages_to_turns`, `mode=complete` skips the loop and strips prompt echo).
- `rag.rs` — `semantic_search` storage forwarding, budgeted prompt assembly (drop-lowest-score, near-limit warning, redactor pass-through/redaction, scanner warn/reject/none), citation enrichment + answer-order renumbering + graceful degradation (uses stub embedder/AI/dispatcher).
- `local_embedding.rs` *(feature)* — cache hit/partial/clear/disable, model-alias mapping, fetch-count mismatch; an `#[ignore]`d round-trip that downloads BGE weights.
- `predict.rs` — `sanitize_completion` cases; handler errors without provider / unsupported provider.
- `ipc.rs` — `AiStreamChatArgs` serde (default shape, complete+stop+trim, unknown-mode rejection).
- `tests/predict_smoke.rs` — env-gated (`NEXUS_TEST_OLLAMA=1`) live Ollama FIM latency smoke against the BL-139 300ms DoD budget; no-ops otherwise.

Gaps: capability-gate enforcement is tested in `nexus-bootstrap`, not here; the `ts-export` bindings and `local-embeddings` real-model path are only exercised under their feature flags / `--ignored`.
