# Application Capabilities

> **As of:** 2026-05-17. Feature inventory of what Nexus does, organized by domain. The mechanics of "which plugin provides what" live in [`plugin-capabilities.md`](plugin-capabilities.md); the security gates (which `Capability` enum value protects which verb) live in [`capabilities.md`](capabilities.md).

This is the user-and-developer-facing answer to "what can Nexus do?" Every line below is wired to live code — citations point at the responsible plugin / handler.

## Notes & content

- **Markdown + GFM + MDX** — CommonMark plus tables, task lists, strikethrough, footnotes, math; MDX components (`<Card />`, `<Callout>`, `<Alert>`, `<Badge>`) registered in `builtins.ts`. Tags, callouts, frontmatter, wikilinks, embeds, block refs. Source: `nexus-formats`, `nexus-editor`.
- **Wikilinks with 3-tier resolution** — exact → basename → case-insensitive stem; bidirectional phantom-link upgrade as files appear. Source: `nexus-storage`.
- **Block IDs** — every block gets a stable id (`com.nexus.editor::stamp_block`); block-level backlinks (`com.nexus.storage::backlinks_to_block`).
- **YAML frontmatter** — reserved keys + plugin-extensible custom fields.
- **Plain-text find/replace** — across files (`com.nexus.storage::replace_in_files`).
- **Atomic writes** — fsync + rename; storage owns the file watcher so external edits flow back through.

## Search

- **Full-text search** — Tantivy index with stemmer, stopwords, mmap. Scoping operators `tag:`, `path:`, `prop:`, `type:` (`com.nexus.storage::search`).
- **Code symbol index** — tree-sitter parses Rust/TS/Python/Go/JS code blocks, indexes symbols (`query_symbol`). BL-114.
- **Find in files** — ripgrep-style content search (`find_in_files`).
- **Tag / task / block queries** — first-class index queries.
- **Recall (RAG)** — vector search with citations and excerpt extraction.

## Knowledge graph

- **Forge-wide entity graph** — entities + typed relations + confidence (`entity_get`, `entity_search`, `entity_relations`, `entity_find_duplicates`).
- **Backlinks / outgoing / unresolved** — first-class.
- **Dream Cycle** — AI infers and proposes new entity relations as low-confidence draft relations; user reviews via the Dream Cycle inbox (`list_draft_relations`). Confidence decays over time (`entity_decay_relations`).

## AI

- **Provider abstraction** — Anthropic, OpenAI, Ollama, llama.cpp; auto-detected from env (`ANTHROPIC_API_KEY` / `OPENAI_API_KEY` / `OLLAMA_BASE_URL`) and overridable in `ai.toml`.
- **Streaming chat** — `stream_chat` / `stream_ask` with cancel.
- **Inline edit prediction** — Mod+Shift+Space in editor → "AI: Complete at cursor"; FIM completion via `predict` (BL-139).
- **RAG with citations** — embeddings (OpenAI text-embedding-3-small, Ollama nomic-embed, or local fastembed bge-small/bge-base); top-K vector query + citation excerpt extraction.
- **Tool loop** — `propose_tool_calls` returns proposed function calls; agent loop dispatches them via IPC. Function tools auto-exposed from registered storage/ai/skills verbs; MCP tools auto-discovered.
- **File / entity enrichment** — `enrich_file`, `enrich_entity`, `infer_entity_relations`.
- **Semantic search** — `semantic_search`.
- **Docs generation** — `generate_docs` (BL-116).
- **Session history** — per-session chat persistence (`session_load`, `session_list`, `session_save`).
- **Privacy / injection policy** — `PrivacyPolicy` (`Off | RedactPii | LocalOnly`), `InjectionPolicy` (`Off | OnDemand | Always`).
- **TLS pinning** — opt-in cert pinning for provider endpoints (BL-102).
- **Unified runtime / scheduler** — `com.nexus.ai.runtime` exposes `submit / cancel / pause / resume / get / list / events / pool_stats / wait_for` for long-running AI tasks (ADR 0028).

## Agents

- **Archetypes** — Writer, Coder, Researcher built-in (`list_archetypes`); user-defined via `list_custom`.
- **Plan + step + execute** — `plan` returns a `Plan`; `session_run` drives the step loop with `round_decide` for mid-stream interaction.
- **Stepwise approval** — chat + TUI show pending steps; user accepts/rejects.
- **Tools** — every `com.nexus.*` handler + every MCP-server tool can be exposed as an agent tool (`list_tools`).
- **Memory** — `memory_record / memory_query / memory_export / memory_prune` (KV-backed per-agent state).
- **Transcripts** — persisted under `<forge>/.forge/agent/transcripts.sqlite`; FTS5 search (`search_transcripts`).
- **Delegation** — `delegate` shim submits a sub-task to the ai-runtime, awaits via `wait_for`.

## Skills

- **`.skill.md` registry** — YAML frontmatter + markdown body; scanned from `<forge>/skills/`.
- **Render / compose / invoke** — pure-string templating, multi-skill composition, executable invocation (`com.nexus.skills::{render, compose, invoke}`).
- **Context-aware listing** — `list_by_context`, `triggered_by` for context-sensitive skill suggestion.
- **Built-in seed** — code-reviewer, daily-journal, meeting-notes, commit-message scaffolds.

## Workflows

- **`.workflow.toml` registry** — trigger / condition / action model.
- **Trigger types** — cron, file_event, manual; webhook + git_event open.
- **Action types** — `ipc_call` (any handler), composable steps with `${trigger.*}` and `${step.*}` interpolation.
- **Cron scheduler** — declared in TOML, evaluated by the workflow plugin.
- **Run history** — per-workflow run log (`run_history`).
- **Digest pipeline** — `run_digest` cron-driven report assembly.

## Bases (databases over markdown)

- **`.bases` TOML schema** — property types (`Title / Select / Date / Number / MultiSelect / People / timestamps`), relations.
- **Views** — Table, Kanban (drag between columns), Calendar (month grid), Gallery.
- **View pipeline** — 14 filter operators + multi-level sort + grouping (`com.nexus.database::apply_view`).
- **Formulas** — pure-compute expression evaluator (`formula_eval`), 64-level recursion cap.
- **Rollups** — aggregations over related records (`compute_rollup`).
- **CSV** — import and export (`csv_import`, `csv_export`).
- **CRUD over `.base` files** — create/update/delete records, properties, views via `com.nexus.storage::base_*` (15 verbs).

## Canvas

- **Obsidian-compatible canvas JSON** — nodes + edges, snap-to-grid.
- **Auto-layout** — `AUTO_LAYOUT_ITERATIONS` constraint solver.
- **Export** — PNG (max 8192 px edge), with margin control.
- **Minimap + overview** — pan/zoom, hit-test handles.
- **Embedded content** — terminal nodes, file previews (32 KB / 64 KB caps).

## Editor

- **CodeMirror 6 surface** — syntax highlighting, keymap, decoration compartments.
- **Block tree** — `Block` + `BlockType`, transaction primitives (insert/delete/merge/split), undo tree.
- **MDX components** — block-form and self-closing; markdown-inside-MDX rendering.
- **Multibuffer** — `open_excerpts` / `refresh_excerpts` assemble read-only multi-file views (BL-141).
- **Block-link resolution** — `resolve_block_link`.
- **Embedded base views** — `execute_database_view` runs a base query inline in the editor.
- **CRDT collaboration** — operation-based CRDT (RGA for text) per BL-074; conflict toast + pick-side resolution.

## Terminal & process manager

- **PTY sessions** — `portable-pty` per-session reader thread; LRU eviction past 50 sessions.
- **Shell detection** — bash/zsh/fish/sh/cmd/pwsh.
- **Profile sourcing** — guarded rc-file inclusion per shell (§1.3).
- **ANSI rendering** — 16/256/truecolor, bold/italic/underline/inverse/strike, CR-rewind, backspace.
- **Saved commands** — slug/icon/shell/working_dir/sidebar_order; sidebar UI; `nexus proc` CLI.
- **Ad-hoc history** — dedupe by `(command, working_dir)`, promotion to saved.
- **Pre-command runner** — sentinel-based exit-code detection per step (POSIX).
- **Memory monitoring** — RSS sample / 60-sample rolling history / `MemoryLimits { soft_mb, hard_mb }`.
- **AI suggestions** — 5 built-in rules (cargo failure → `cargo check --message-format=json`, npm not-found → `npm update`, shell command-not-found → `which`, ssh permission-denied → `ssh-add -l`, port-in-use → `lsof -iTCP`).
- **REPL sessions** — `repl_start / repl_eval / repl_stop / repl_list` (BL-142).
- **Cross-session search** — FTS across every session's output.
- **Job objects** — `KILL_ON_JOB_CLOSE` + `BREAKAWAY_OK` on Windows.

## Git

- **38 IPC verbs** spanning status / log / branches / blame / diff / hunks / stage / commit / push / branch+tag CRUD / merge+rebase+cherry-pick + abort / stash / conflicts / LFS.
- **Hunk staging** — `stage_hunks` / `unstage_hunks` / `discard_hunks`.
- **Auto-committer** — opt-in periodic auto-commit.
- **Live state events** — `com.nexus.git.{state, branch_changed, commit, dirty_changed}` topics.
- **SSH passphrase caching** — keyring-backed via `nexus git set-passphrase / clear-passphrase` (BL-090).
- **Conflict tooling** — enumeration + 3-way version fetch (ours/theirs/base).

## LSP / DAP / ACP / MCP host

- **LSP** — multi-server host; spawns servers, bridges Content-Length framed JSON-RPC; verbs: completions, hover, definition, references, code_actions, format, rename, execute_command. BL-076.
- **DAP** — debug adapter host; launch / attach + full session control (continue/next/step in/step out/pause, threads, stack_trace, scopes, variables, evaluate, breakpoint sets). BL-081.
- **ACP** — agent client protocol host; spawn agent process + propose/accept/reject. BL-144.
- **MCP host** — connect external MCP servers; list tools/prompts/resources; call_tool; dynamic-tool registry write surface. Streamable-HTTP + stdio transports.
- **Plugin contributions** — community plugins can contribute adapters/servers via `[protocol_hosts]` manifest block (BL-113); gated by `protocol.host.contribute` cap.

## MCP server

- **`nexus mcp serve`** stdio MCP server exposing the forge as 15 `nexus_*` tools; usable from Claude Code / Cursor / any MCP client.
- **Dynamic tools** — plugins register additional MCP tools at runtime (`register_tool`).
- **OAuth** — for MCP servers that need it (`auth.rs`, 30 s timeout).

## Notifications

- **Channels** — desktop toast, Discord webhook, Telegram bot (4096-byte split), email (SMTP via `lettre` with rustls + ring).
- **Inbox** — SQLite-backed notification inbox at `<forge>/.forge/notifications/inbox.db`; max 1000 rows / 30-day default retention.
- **Routing** — `[sources.<id>]` declares which kernel-bus topics fan out to which channels.
- **Inbox UI** — list / stats / mark_read / dismiss.

## Audio

- **STT (Speech-to-Text)** — local Whisper backend + OpenAI provider backend; opt-in `audio.record` cap.
- **TTS (Text-to-Speech)** — synthesize verb; bundled provider model URLs (whisper.cpp ggml from HuggingFace).
- **Model dir** — local cache; download flow via the configured model URL.

## Comments

- **Block-anchored threads** — JSON sidecar per file; persistent across edits.
- **Thread CRUD** — `create_thread`, `add_reply`, `edit_comment`, `delete_comment`, `delete_thread`, `set_resolved`.

## Theming

- **CSS variable registry** — ~547 tokens (`--nx-*`).
- **11 bundled themes** including `nexus-manuscript` warm sepia.
- **Snippet cascade** — toggle + reorder; `set_plugin_overrides` for plugin-contributed themes.
- **Theme picker plugin** — Themes / Snippets / Build tabs with live colour picker + TOML export.
- **Forge-local** — theme config persisted under `<forge>/.forge/`.

## Templates

- **`.template.md`** — parameter substitution; user-templates under `<forge>/.forge/templates/`; built-ins bundled.
- **Render / apply** — `render` to string; `apply` writes via storage.

## Plugin system

- **Three runtimes** — Native (in-tree only), Wasm (community), Script/JS (community, iframe sandbox).
- **WASM sandbox** — wasmtime + fuel (10M default) + epoch deadline (5 s default) + memory cap (16 MB default).
- **JS sandbox** — iframe + orchestrator-bound `pluginId` (F-8.1.2).
- **Capability gating** — manifest declares required + optional; user grants at install; encrypted `granted_caps.json` per plugin (chacha20poly1305, key in OS keyring; BL-101).
- **Manifest signing** — ed25519 (BL-099); enforced when `KernelConfig.require_signatures = true`.
- **Hot reload** — `notify-debouncer-mini` watch; rollback on failure; 3-strike crash quarantine.
- **Scaffolding** — `nexus plugin scaffold --type wasm|script <id>`.

## Security

- **Credential vault** — OS keyring per-plugin namespace (`{plugin_id}:{name}`); cross-plugin reads blocked.
- **Audit log** — every cap-gated call → `<forge>/.forge/.kernel/audit.db`; 90-day retention; queryable via `query_audit_log`.
- **TLS pinning** — opt-in cert pinning for AI providers (BL-102).
- **Path validation** — TOCTOU + traversal fixes in `nexus-storage`.
- **Sandbox isolation** — fuel + deadline + memory + per-call cap check.
- **Capability matrix** — every IPC handler classified in `cap_matrix.toml`; integration test enforces completeness.

## Multi-frontend

- **CLI** (`nexus`) — 18+ subcommand groups: `forge / content / plugin / ai / agent / skill / workflow / proc / term / mcp / bases / canvas / config / db / git / graph / logs / watch / collab / notify / tool / desktop`. Output formats: json, jsonl, text, table. Plugin external subcommand dispatch.
- **TUI** (`nexus-tui`) — ratatui; modes for editor, terminal, agent, git; magenta `TERM` badge in status; raw input forwarding.
- **MCP server** — `nexus mcp serve` — stdio MCP exposing forge tools.
- **Tauri desktop shell** — Hosts the React frontend; thin Tauri bridge (25 commands).
- **Remote forge** — `nexus-remote` exposes the full kernel IPC + event-bus over JSON-RPC; SSH child-process transport via `--forge-path ssh://...`. Reconnect-on-drop wrapper.

## Collaboration

- **Live presence** — `publish_presence` publishes caret/selection on the kernel bus.
- **WebSocket relay** — `start_relay` binds 0.0.0.0; `stop_relay`, `relay_status`. User-initiated through the collab panel.
- **CRDT sync** — operation-based RGA; per-block conflict resolver toast (manual side-pick).
- **Git merge driver shim** — CRDT-aware merge during pull-landing.

## Link preview

- **OG + Twitter card fetcher** — 5 s timeout, 512 KB max body.

## Cross-platform / portability

- **Linux / macOS / Windows** — `process.spawn` paths are per-OS; Job Objects on Windows, process-group kill on Unix.
- **WSLg support** — `WEBKIT_DISABLE_*` + `GDK_BACKEND=x11` baked into `shell/`'s `tauri:dev` for WSL2.
- **Auto-updater + Sentry** — deferred per personal-tool scope.

## What Nexus deliberately does **not** do (at v0.1.2)

- **Cloud sync** — file-as-truth means whatever sync layer you point at the forge dir works (git, syncthing, dropbox). No first-party sync service.
- **Account system** — no login; identity comes from git config or `[collab]` peer id.
- **Mobile** — UniFFI mobile binding deferred.
- **Web/OPFS** — deferred.
- **Cross-database relation queries** — rollup / lookup resolvers route through `com.nexus.storage`; no SQL JOIN across bases.
- **Marketplace + auto-update** — manual install only at v0.1.2.

## How to extend a capability

Pick the capability domain above → find the owning plugin in [`plugin-capabilities.md`](plugin-capabilities.md) → add an IPC handler in that crate → add a row in `cap_matrix.toml` → consume via `ctx.ipc_call(...)`. Walkthrough: [`architecture.md` → How to add a backend capability](architecture.md#how-to-add-a-backend-capability-the-right-way).
