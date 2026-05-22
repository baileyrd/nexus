# Hardcoded Values — Rust Side

> **As of:** 2026-05-21. Companion to [`hardcoded-shell.md`](hardcoded-shell.md) (shell-side) and [`plugin-manifest-defaults.md`](plugin-manifest-defaults.md) (manifest-baked defaults). Items here are candidates for promotion to a setting (**User Config**) or at minimum a named constant (**Dev Config**). Citations are `crate/path/file.rs:line` relative to repo root.

Workflow: pick an item from this list, promote it to a config field or named constant, then delete the row.

---

## User Config

Things that belong in a user-facing settings UI.

### Network endpoints

P2-05 (2026-05-17). The originally-flagged rows are crossed below with their landed promotions.

~~| `crates/nexus-ai/src/ollama.rs` | 15 | `"http://localhost:11434"` | `ai.ollama_base_url` |~~ → `nexus_formats::AiConfig.ollama_base_url`; runtime falls through to `nexus_ai::ollama::DEFAULT_BASE_URL`.
~~| `crates/nexus-audio/src/local_backend.rs` | 105 | `"https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin"` | `audio.whisper_model_url` |~~ → `[audio] whisper_model_url = "..."` in `config.toml`; default `nexus_audio::config::DEFAULT_WHISPER_MODEL_URL_TEMPLATE`.
~~| `crates/nexus-audio/src/provider_backend.rs` | 43 | `"https://api.openai.com"` | `audio.openai_api_base_url` |~~ → already overridable via `[audio] provider_base_url`; default now `pub nexus_audio::provider_backend::DEFAULT_BASE_URL`.
~~| `crates/nexus-bootstrap/src/collab.rs` | 233 | `"ws://relay:7700/"` | `collab.relay_url` |~~ → already per-forge in `[collab] relay_url`; the cited line was a test literal, not a default.
~~| `crates/nexus-cli/src/commands/collab.rs` | 104 | `"0.0.0.0:{port}"` | `collab.bind_address` |~~ → new `--bind <ip>` flag on `nexus collab serve`; default `DEFAULT_BIND_ADDRESS = "0.0.0.0"`.
~~| `crates/nexus-cli/src/main.rs` | 826 | `7700` (default port) | `collab.default_port` |~~ → `--port` flag already exists; constant is `commands::collab::DEFAULT_SERVE_PORT`.

### AI / ML model defaults

P2-04 (2026-05-17) promoted the per-provider model and FIM-temperature defaults to `ai.toml`. The provider-level constants stay as ultimate fallbacks (`nexus_ai::{anthropic,openai,ollama}::DEFAULT_*`) but a forge can now override each via the `[ai]` table — see [`forge-config.md`](forge-config.md#forgeforgeai.toml).

~~| `crates/nexus-ai/src/anthropic.rs` | 13 | `"claude-sonnet-4-20250514"` | `ai.anthropic_model` |~~
~~| `crates/nexus-ai/src/openai.rs` | 14 | `"gpt-4o"` | `ai.openai_chat_model` |~~
~~| `crates/nexus-ai/src/openai.rs` | 17 | `"text-embedding-3-small"` | `ai.openai_embedding_model` |~~
~~| `crates/nexus-ai/src/ollama.rs` | 18 | `"llama3.2"` | `ai.ollama_chat_model` |~~
~~| `crates/nexus-ai/src/ollama.rs` | 21 | `"nomic-embed-text"` | `ai.ollama_embedding_model` |~~
~~| `crates/nexus-ai/src/ollama.rs` | 198 | `0.2` (temperature) | `ai.ollama_temperature` |~~

Still open:

| File | Line | Value | Suggested setting key |
|------|------|-------|----------------------|
| `crates/nexus-ai/src/local_embedding.rs` | 44 | `"bge-small-en-v1.5-int8"` | already env-overridable; surface in settings UI |

> `AiConfig.model`/`max_tokens`/`context_window` are already user-tunable in `ai.toml`; the per-provider defaults above were promoted in P2-04.

### Operation timeouts (user-perceivable)

P2-06 (2026-05-17). Each value is now exposed as a `pub const DEFAULT_*` so it's discoverable from a single grep, even when no runtime override path is wired yet. Where a Config struct already receives the value (audio, collab bootstrap), the override is also live; where it doesn't (MCP, git, storage, theme, ai), the const is the only source today — a future `[mcp.timeouts]` / `[git.timing]` / etc. block is the next step.

~~| `crates/nexus-mcp/src/client.rs` | 79 | `Duration::from_secs(15)` | `mcp.connect_timeout_secs` |~~ → `nexus_mcp::client::DEFAULT_CONNECT_TIMEOUT`.
~~| `crates/nexus-mcp/src/client.rs` | 83 | `Duration::from_secs(5)` | `mcp.shutdown_timeout_secs` |~~ → `nexus_mcp::client::DEFAULT_SHUTDOWN_TIMEOUT`.
~~| `crates/nexus-mcp/src/server.rs` | 42 | `Duration::from_secs(30)` | `mcp.ipc_timeout_secs` |~~ → `nexus_mcp::server::DEFAULT_IPC_TIMEOUT`.
~~| `crates/nexus-mcp/src/server.rs` | 45 | `Duration::from_secs(120)` | `mcp.ai_ipc_timeout_secs` |~~ → `nexus_mcp::server::DEFAULT_AI_IPC_TIMEOUT`.
~~| `crates/nexus-mcp/src/auth.rs` | 322 | `Duration::from_secs(30)` | `mcp.oauth_timeout_secs` |~~ → `nexus_mcp::auth::DEFAULT_OAUTH_TIMEOUT`.
~~| `crates/nexus-collab/src/client.rs` | 77 | `Duration::from_secs(10)` | `collab.handshake_timeout_secs` |~~ → already `nexus_collab::DEFAULT_HANDSHAKE_TIMEOUT`; forward-compatible `[collab] handshake_timeout_secs` field added to bootstrap config (pending ReconnectingClient surfacing the knob).
~~| `crates/nexus-collab/src/reconnect_client.rs` | 69 | `30 s` (backoff max) | `collab.backoff_max_secs` |~~ → already pluggable via `[collab] max_delay_ms` in `config.toml`.
~~| `crates/nexus-collab/src/reconnect_client.rs` | 84 | `2.0` (factor) | `collab.backoff_factor` |~~ → new `[collab] backoff_factor` field threaded into `ReconnectConfig`.
~~| `crates/nexus-git/src/core_plugin.rs` | 178 | `Duration::from_secs(2)` | `git.poll_interval_secs` |~~ → `nexus_git::core_plugin::DEFAULT_POLL_INTERVAL`.
~~| `crates/nexus-git/src/core_plugin.rs` | 980 | `Duration::from_secs(30)` | `git.auto_commit_tick_secs` |~~ → `nexus_git::core_plugin::DEFAULT_AUTO_COMMIT_TICK`.
~~| `crates/nexus-storage/src/core_plugin.rs` | 32 | `Duration::from_millis(500)` | `storage.git_commit_poll_interval_ms` |~~ → `nexus_storage::core_plugin::DEFAULT_GIT_COMMIT_POLL_INTERVAL`.
~~| `crates/nexus-theme/src/watcher.rs` | 25 | `500` ms | `ui.theme_debounce_ms` |~~ → already `nexus_theme::watcher::DEFAULT_DEBOUNCE_MS`.
~~| `crates/nexus-ai/src/indexing_daemon.rs` | 42 | `Duration::from_secs(2)` | `ai.indexing_debounce_secs` |~~ → already `nexus_ai::indexing_daemon::DEFAULT_DEBOUNCE`.
~~| `crates/nexus-audio/src/provider_backend.rs` | 44 | `Duration::from_secs(2)` | `audio.creds_lookup_timeout_secs` |~~ → live: `[audio] creds_lookup_timeout_secs = N`; default `nexus_audio::provider_backend::DEFAULT_CREDS_LOOKUP_TIMEOUT`.

### Notification limits

All three items below were promoted in P2-07 (2026-05-17). The Telegram cap moved to `[channels.telegram].max_bytes` in `notifications.toml`; the inbox row + age caps were already pluggable under `[inbox]` and are now documented in [`forge-config.md`](forge-config.md#forgeforgnotificationstoml).

~~| `crates/nexus-notifications/src/lib.rs` | 378 | `4096` bytes (Telegram message split) | `notifications.telegram_max_bytes` |~~
~~| `crates/nexus-notifications/src/inbox.rs` | 47 | `1000` rows | `notifications.inbox_max_rows` |~~
~~| `crates/nexus-notifications/src/inbox.rs` | 50 | `30` days | `notifications.inbox_max_age_days` |~~

---

## Dev Config

Named-constant candidates. Not user-facing — internal performance / protocol tuning that should still be expressed as a named `const` rather than an inline literal.

### IPC + service timeouts (a lot of duplication — consolidation candidate)

Most CLI subcommands declare their own ipc-call timeout as a per-file local. Many subsystems share the same `Duration::from_secs(30)` / `Duration::from_secs(60)` / `Duration::from_secs(120)`. A shared `nexus-config` (or even `nexus-types`) module of standard timeouts would deduplicate ~30 entries.

**Phase 5 P5-01 (2026-05-18):** Shared bucket constants now live at `crates/nexus-types/src/constants.rs` (`IPC_TIMEOUT_SHORT` 30s, `IPC_TIMEOUT_NORMAL` 60s, `IPC_TIMEOUT_LONG` 120s, `IPC_TIMEOUT_EXTENDED` 600s). All `crates/nexus-cli/src/commands/*.rs` per-file `IPC_TIMEOUT` literals now alias the shared bucket. Remaining rows below are subsystem-side timeouts that did not migrate in P5-01.

| File | Line | Value | Constant name |
|------|------|-------|---------------|
~~| `crates/nexus-cli/src/commands/ai.rs` | 19 | `Duration::from_secs(120)` | `AI_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_LONG`.
~~| `crates/nexus-cli/src/commands/logs.rs` | 11 | `Duration::from_secs(30)` | `LOGS_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_SHORT`.
~~| `crates/nexus-cli/src/commands/notify.rs` | 25 | `Duration::from_secs(15)` | `NOTIFY_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_SHORT` (relaxed 15s → 30s).
~~| `crates/nexus-cli/src/commands/db.rs` | 22 | `Duration::from_secs(30)` | `DB_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_SHORT`.
~~| `crates/nexus-cli/src/commands/tool.rs` | 14 | `Duration::from_secs(10)` | `TOOL_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_SHORT` (relaxed 10s → 30s).
~~| `crates/nexus-cli/src/commands/agent.rs` | 36 | `Duration::from_secs(600)` | `AGENT_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_EXTENDED`.
~~| `crates/nexus-cli/src/commands/proc.rs` | 16 | `Duration::from_secs(30)` | `PROC_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_SHORT`.
~~| `crates/nexus-cli/src/commands/workflow.rs` | 14 | `Duration::from_secs(30)` | `WORKFLOW_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_SHORT`.
~~| `crates/nexus-cli/src/commands/workflow.rs` | 16 | `Duration::from_secs(600)` | `WORKFLOW_RUN_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_EXTENDED`.
~~| `crates/nexus-cli/src/commands/mcp.rs` | 15 | `Duration::from_secs(60)` | `MCP_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_NORMAL`.
~~| `crates/nexus-cli/src/commands/skill.rs` | 14 | `Duration::from_secs(30)` | `SKILL_IPC_TIMEOUT_SECS` |~~ → `nexus_types::constants::IPC_TIMEOUT_SHORT`.
| `crates/nexus-cli/src/commands/term.rs` | 80 | `Duration::from_millis(200)` | `TTY_READ_TIMEOUT_MS` |
| `crates/nexus-cli/src/commands/term.rs` | 99 | `Duration::from_millis(100)` | `TTY_READ_SHORT_TIMEOUT_MS` |
| `crates/nexus-cli/src/commands/term.rs` | 126 | `Duration::from_millis(500)` | `SHUTDOWN_REQUEST_TIMEOUT_MS` |
| `crates/nexus-editor/src/core_plugin.rs` | 42 | `Duration::from_secs(30)` | `STORAGE_IPC_TIMEOUT_SECS` |
| `crates/nexus-skills/src/core_plugin.rs` | 188 | `Duration::from_secs(120)` | `INVOKE_AGENT_TIMEOUT_SECS` |
| `crates/nexus-lsp/src/client.rs` | 118 | `Duration::from_secs(10)` | `LSP_DEFAULT_REQUEST_TIMEOUT_SECS` |
| `crates/nexus-lsp/src/client.rs` | 121 | `Duration::from_secs(30)` | `LSP_INITIALIZE_TIMEOUT_SECS` |
| `crates/nexus-lsp/src/core_plugin.rs` | 311 | `Duration::from_secs(5)` | `LSP_SHUTDOWN_DEADLINE_SECS` |
| `crates/nexus-remote/src/client.rs` | 32 | `Duration::from_secs(600)` | `REMOTE_DEFAULT_CALL_TIMEOUT_SECS` |
| `crates/nexus-remote/src/server.rs` | 33 | `Duration::from_secs(600)` | `REMOTE_DISPATCH_TIMEOUT_SECS` |
| `crates/nexus-remote/src/server.rs` | 37 | `Duration::from_secs(3600)` | `REMOTE_MAX_DISPATCH_TIMEOUT_SECS` |
| `crates/nexus-acp/src/client.rs` | 119 | `Duration::from_secs(60)` | `ACP_REQUEST_TIMEOUT_SECS` |
| `crates/nexus-acp/src/client.rs` | 122 | `Duration::from_secs(30)` | `ACP_INITIALIZE_TIMEOUT_SECS` |
| `crates/nexus-acp/src/server.rs` | 44 | `Duration::from_secs(600)` | `ACP_DISPATCH_TIMEOUT_SECS` |
| `crates/nexus-dap/src/client.rs` | 182 | `Duration::from_secs(10)` | `DAP_REQUEST_TIMEOUT_SECS` |
| `crates/nexus-dap/src/client.rs` | 185 | `Duration::from_secs(20)` | `DAP_INITIALIZE_TIMEOUT_SECS` |
| `crates/nexus-dap/src/core_plugin.rs` | 342 | `Duration::from_secs(5)` | `DAP_SHUTDOWN_DEADLINE_SECS` |
| `crates/nexus-mcp/src/core_plugin.rs` | 325 | `Duration::from_secs(5)` | `MCP_SHUTDOWN_DEADLINE_SECS` |
| `crates/nexus-mcp/src/core_plugin.rs` | 327 | `Duration::from_millis(50)` | `MCP_POLL_INTERVAL_MS` |
| `crates/nexus-mcp/src/client.rs` | 459 | `Duration::from_millis(500)` | `MCP_SHORT_CONNECT_TIMEOUT_MS` |
| `crates/nexus-mcp/src/client.rs` | 569 | `Duration::from_millis(2_000)` | `MCP_RETRY_CONNECT_TIMEOUT_MS` |
| `crates/nexus-agent/src/auto_notify.rs` | 27 | `Duration::from_secs(5)` | `AGENT_NOTIFY_TIMEOUT_SECS` |
| `crates/nexus-agent/src/handlers/delegate.rs` | 78 | `Duration::from_secs(30)` | `AGENT_SUBMIT_TIMEOUT_SECS` |
| `crates/nexus-agent/src/handlers/delegate.rs` | 88 | `Duration::from_secs(10_800)` | `AGENT_WAIT_FOR_TIMEOUT_SECS` (3h) |
| `crates/nexus-agent/src/handlers/shared.rs` | 26 | `Duration::from_secs(60)` | `AGENT_DEFAULT_TOOL_TIMEOUT_SECS` |
| `crates/nexus-agent/src/handlers/shared.rs` | 29 | `Duration::from_secs(300)` | `AGENT_DEFAULT_CHAT_TIMEOUT_SECS` |
~~| `crates/nexus-tui/src/app.rs` | 1820 | `Duration::from_secs(600)` | `TUI_AGENT_IPC_TIMEOUT_SECS` |~~ → already `nexus_tui::app::AGENT_IPC_TIMEOUT` (`app.rs:1821`).
~~| `crates/nexus-tui/src/app.rs` | 1825 | `Duration::from_secs(1800)` | `TUI_MODAL_AUTO_REJECT_TIMEOUT_SECS` |~~ → already `nexus_tui::app::MODAL_AUTO_REJECT_TIMEOUT` (`app.rs:1826`).
| `crates/nexus-ai/src/vectorstore.rs` / `rag.rs` / `tools/functions.rs` / `generate_docs.rs` | various | mostly `Duration::from_secs(30)` | shared `AI_STORAGE_IPC_TIMEOUT_SECS` |
| `crates/nexus-ai/src/tools/mcp_bridge.rs` | 44 | `Duration::from_secs(5)` | `AI_MCP_DISCOVERY_TIMEOUT_SECS` |
| `crates/nexus-ai/src/tools/mcp_bridge.rs` | 49 | `Duration::from_secs(60)` | `AI_MCP_CALL_TIMEOUT_SECS` |
| `crates/nexus-bootstrap/src/storage.rs` / `terminal.rs` / `database.rs` / `remote.rs` | 25-32 | `Duration::from_secs(30)` | shared `BOOTSTRAP_IPC_TIMEOUT_SECS` |
| `crates/nexus-bootstrap/src/dream_cycle.rs` | 61 | `Duration::from_secs(120)` | `DREAM_CYCLE_IPC_TIMEOUT_SECS` |
| `crates/nexus-bootstrap/src/crdt_publisher.rs` | 47 | `Duration::from_millis(250)` | `CRDT_PULL_LANDING_TICK_MS` |
| `crates/nexus-bootstrap/src/{lsp,mcp,dap,acp}_contribution_wiring.rs` | ~28 each | `Duration::from_secs(5)` | shared `PROTO_REGISTER_TIMEOUT_SECS` |
| `crates/nexus-bootstrap/src/agent.rs` | 27, 32 | `60` / `300` s | `AGENT_DEFAULT_TOOL_TIMEOUT_SECS` / `_CHAT_TIMEOUT_SECS` |
| `crates/nexus-workflow/src/digests.rs` | 49 | `Duration::from_secs(120)` | `DIGEST_IPC_TIMEOUT_SECS` |

### Connection pool backoff

| File | Line | Value | Constant name |
|------|------|-------|---------------|
| `crates/nexus-mcp/src/pool.rs` | 42-46 | `[100 ms, 500 ms, 2 s, 10 s, 30 s]` | `DEFAULT_BACKOFF_SCHEDULE` |
| `crates/nexus-mcp/src/pool.rs` | 71 | `Duration::from_secs(300)` | `DEFAULT_IDLE_TIMEOUT_SECS` |
| `crates/nexus-mcp/src/pool.rs` | 72 | `Duration::from_secs(30)` | `DEFAULT_CONNECT_TIMEOUT_SECS` |

### Buffer / message size caps

| File | Line | Value | Constant name |
|------|------|-------|---------------|
~~| `crates/nexus-collab/src/client.rs` | 72 | `16 * 1024 * 1024` | `MAX_FRAME_BYTES` |~~ → already `nexus_collab::client::MAX_FRAME_BYTES`.
~~| `crates/nexus-collab/src/server.rs` | 40 | `1024` | `BROADCAST_CAPACITY` |~~ → already `nexus_collab::server::BROADCAST_CAPACITY`.
~~| `crates/nexus-collab/src/server.rs` | 45 | `16 * 1024 * 1024` | `MAX_FRAME_BYTES` |~~ → already `nexus_collab::server::MAX_FRAME_BYTES`.
| `crates/nexus-collab/src/reconnect_client.rs` | 305 | `16 * 1024 * 1024` | (already explicit in ws_config) |
| `crates/nexus-formats/src/notion/database.rs` | 14 | `256` | `SAMPLE_LIMIT` |
| `crates/nexus-formats/src/markdown/frontmatter.rs` | 15 | `256 * 1024` | `MAX_FRONTMATTER_BYTES` |
| `crates/nexus-formats/src/markdown/embed.rs` | 10 | `10` | `MAX_EMBED_DEPTH` |
| `crates/nexus-formats/src/util/filename.rs` | 6 | `255` | `MAX_FILENAME_BYTES` |
| `crates/nexus-formats/src/util/filename.rs` | 9 | `260` | `MAX_PATH_BYTES` |
| `crates/nexus-formats/src/canvas/mod.rs` | 20 | `50 * 1024 * 1024` | `MAX_CANVAS_BYTES` |
| `crates/nexus-formats/src/canvas/mod.rs` | 26 | `100_000` | `MAX_CANVAS_ELEMENTS` |
~~| `crates/nexus-mcp/src/core_plugin.rs` | 551 | `4 * 1024 * 1024` | `MAX_TOOL_RESPONSE_BYTES` |~~ → already `MAX_TOOL_RESPONSE_BYTES` at `core_plugin.rs:576`.
~~| `crates/nexus-mcp/src/core_plugin.rs` | 552 | `1024` | `MAX_TOOL_RESPONSE_ITEMS` |~~ → already `MAX_TOOL_RESPONSE_ITEMS` at `core_plugin.rs:577`.
~~| `crates/nexus-editor/src/core_plugin.rs` | 2220 | `16 * 1024 * 1024` | `MAX_TRANSACTION_BYTES` |~~ → already `MAX_TRANSACTION_BYTES` at `handlers/transaction.rs:25`.
~~| `crates/nexus-editor/src/core_plugin.rs` | 923 | `500` ops | `UNDO_PERSIST_MAX_OPS` |~~ → already `UNDO_PERSIST_MAX_OPS` at `handlers/session.rs:291`.
| `crates/nexus-lsp/src/transport.rs` | 127 | `16 * 1024 * 1024` | `MAX_BODY_BYTES` |
| `crates/nexus-remote/src/transport.rs` | 21 | `16 * 1024 * 1024` | `MAX_LINE_BYTES` |
| `crates/nexus-acp/src/transport.rs` | 25 | `16 * 1024 * 1024` | `MAX_LINE_BYTES` |
| `crates/nexus-dap/src/transport.rs` | 58 | `16 * 1024 * 1024` | `MAX_BODY_BYTES` |
| `crates/nexus-storage/src/code_index.rs` | 147 | `200` | `DEFAULT_QUERY_LIMIT` |
| `crates/nexus-storage/src/find_replace.rs` | 53 | `200` | `DEFAULT_MAX_FILES` |
| `crates/nexus-storage/src/find_replace.rs` | 57 | `1_000` | `DEFAULT_MAX_RESULTS` |
~~| `crates/nexus-ai/src/vectorstore.rs` | 72 | `4096` | `MAX_CHUNKS_PER_UPSERT` |~~ → already `pub const MAX_CHUNKS_PER_UPSERT`.
~~| `crates/nexus-ai/src/vectorstore.rs` | 77 | `256 * 1024` | `MAX_CHUNK_TEXT_BYTES` |~~ → already `pub const MAX_CHUNK_TEXT_BYTES`.
~~| `crates/nexus-ai/src/vectorstore.rs` | 83 | `12_288` | `MAX_EMBEDDING_DIM` |~~ → already `pub const MAX_EMBEDDING_DIM`.
~~| `crates/nexus-ai/src/indexing_daemon.rs` | 46 | `32` | `DEFAULT_MAX_BATCH` |~~ → already `pub const DEFAULT_MAX_BATCH`.
~~| `crates/nexus-ai/src/rag.rs` | 29 | `1024` | `DEFAULT_MAX_CHUNK_SIZE` |~~ → already `const DEFAULT_MAX_CHUNK_SIZE`.
~~| `crates/nexus-ai/src/rag.rs` | 34 | `200` | `CITATION_EXCERPT_MAX_CHARS` |~~ → already `const CITATION_EXCERPT_MAX_CHARS`.
| `crates/nexus-database/src/formula/eval.rs` | 105 | `64` | `MAX_RECURSION_DEPTH` |
| `crates/nexus-database/src/core_plugin.rs` | 235 | `10 * 1024 * 1024` | `MAX_CSV_IMPORT_BYTES` |
| `crates/nexus-workflow/src/webhook.rs` | 54 | `16 * 1024` | `MAX_HEADER_BYTES` |
| `crates/nexus-workflow/src/run_history.rs` | 30 | `200` | `RUN_HISTORY_CAP` |
| `crates/nexus-bootstrap/src/audit_sqlite.rs` | 14 | `1000` | `DEFAULT_QUERY_LIMIT` |
~~| `crates/nexus-ai/src/enrichment.rs` | 63 | `1500` | `MAX_QUERY_CHARS` |~~ → already `const MAX_QUERY_CHARS`.
~~| `crates/nexus-ai/src/enrichment.rs` | 69 | `200` | `MAX_SUMMARY_CHARS` |~~ → already `const MAX_SUMMARY_CHARS`.

### Vector embedding dimensions

| File | Line | Value | Constant name |
|------|------|-------|---------------|
| `crates/nexus-ai/src/openai.rs` | 20 | `1536` (text-embedding-3-small) | `OPENAI_EMBEDDING_DIM` |
| `crates/nexus-ai/src/ollama.rs` | 24 | `768` (nomic-embed) | `OLLAMA_EMBEDDING_DIM` |
| `crates/nexus-ai/src/local_embedding.rs` | 284 | `768` (bge-base / nomic) | `LOCAL_EMBEDDING_DIM_BASE` |
| `crates/nexus-ai/src/local_embedding.rs` | 286 | `384` (bge-small) | `LOCAL_EMBEDDING_DIM_SMALL` |

### Retry / loop counts

| File | Line | Value | Constant name |
|------|------|-------|---------------|
| `crates/nexus-notifications/src/inbox.rs` | 705 | `for _ in 0..5` | `INBOX_RETRY_ATTEMPTS` |
| `crates/nexus-crdt/src/sync.rs` | 310 | `for _ in 0..32` | `SYNC_LOOP_ATTEMPTS` |
| `crates/nexus-audio/src/local_backend.rs` | 402 | `for _ in 0..100` | `AUDIO_LOOP_ITERATIONS` |
| `crates/nexus-audio/src/local_backend.rs` | 446 | `for _ in 0..800` | `AUDIO_BUFFER_ITERATIONS` |

### Filesystem paths

| File | Line | Value | Constant name |
|------|------|-------|---------------|
| `crates/nexus-notifications/src/lib.rs` | 84 | `".forge/notifications/inbox.db"` | `INBOX_DB_RELPATH` |
| `crates/nexus-notifications/src/lib.rs` | 94 | `".forge/notifications.toml"` | `NOTIFICATIONS_CONFIG_RELPATH` |
| `crates/nexus-crdt/src/state.rs` | 106 | `".forge/.editor/crdt/{hex}.json"` | (computed) — split into `CRDT_STATE_DIR` |

### IPC plugin name literals

These appear in many places — a shared `PLUGIN_IDS` module would centralize them.

| File | Line | Value | Constant name |
|------|------|-------|---------------|
| `crates/nexus-mcp/src/server.rs` | 29-41 | `"com.nexus.storage" / "...ai" / "...skills" / "...git" / "...security"` | `STORAGE_PLUGIN` / `AI_PLUGIN` / etc. |
| `crates/nexus-notifications/src/core_plugin.rs` | 57 | `"com.nexus.notifications"` | `PLUGIN_ID` |
| `crates/nexus-notifications/src/core_plugin.rs` | 74 | `"com.nexus.ai.runtime."` | `AI_RUNTIME_TOPIC_PREFIX` |
| `crates/nexus-crdt/src/wire.rs` | 37 | `"com.nexus.editor.ops."` | `OPS_TOPIC_PREFIX` |
| `crates/nexus-crdt/src/wire.rs` | 44 | `"com.nexus.editor.crdt.conflict."` | `CONFLICT_TOPIC_PREFIX` |

### Miscellaneous

| File | Line | Value | Constant name |
|------|------|-------|---------------|
| `crates/nexus-plugins/src/host_fns.rs` | 24 | `-1001` | `HOST_CAPABILITY_DENIED` |
| `crates/nexus-plugins/src/host_fns.rs` | 27 | `-1002` | `HOST_BUFFER_OVERFLOW` |
| `crates/nexus-plugins/src/manifest.rs` | 1765 | `1024` | `MAX_REGISTRATIONS_PER_KIND` |
| `crates/nexus-editor/src/block.rs` | 606 | `[0.3, 0.7]` | `DEFAULT_SPLIT_RATIOS` |
| `crates/nexus-dap/src/core_plugin.rs` | 61 | `6` | `HANDLER_TERMINATE` |

---

## Already-named constants worth promoting

Existing `const` declarations that are good named constants but should be exposed for user-level or runtime override:

1. `crates/nexus-bootstrap/src/agent.rs:27-32` — `DEFAULT_TOOL_TIMEOUT` / `DEFAULT_CHAT_TIMEOUT` — surface in agent settings UI.
2. `crates/nexus-mcp/src/pool.rs:38-46` — `default_backoff()` schedule — could be per-server config for aggressive/lenient retry policies.
3. `crates/nexus-ai/src/handlers/predict.rs:33-37` — `DEFAULT_MAX_TOKENS` for FIM completions (`64`) — surface as `ai.predict_max_tokens`.
4. `crates/nexus-collab/src/reconnect_client.rs:69-84` — `ReconnectConfig` — already struct, but constructor values are baked.
5. `crates/nexus-formats/src/config/ai.rs:78-79` — `default_max_tokens()` / `default_temperature()` — already serde defaults; consolidate in a single config site.
6. `crates/nexus-ai/src/local_embedding.rs:279-286` — `model_dimension()` hardcoded mapping — move to a model registry table.

---

## Summary

| Track | Count |
|-------|------:|
| User Config | ~30 entries |
| Dev Config | ~100 entries |
| Already-named, surface as setting | 6 |

## Cross-references with shell side

These values also appear shell-side and should be **unified** with the Rust definition rather than duplicated:

- `ai.max_tokens`, `ai.temperature`, default model strings
- `.forge/notifications/...` path literals
- IPC plugin id strings (used by ExtensionHost and shell-side IPC dispatcher)

See [`hardcoded-shell.md`](hardcoded-shell.md) for shell-side equivalents.
