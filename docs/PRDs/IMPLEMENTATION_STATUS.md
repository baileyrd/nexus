# Nexus PRD Implementation Status

> **Snapshot date:** 2026-04-17 (end-of-day — PRD-09 phases M–V landed; Tauri desktop terminal surface live)
> **Scope:** PRDs 01–17 in this directory, audited against `crates/**` and `app/src/**`.
> **Update cadence:** refresh when a PRD's status tier changes, or at minimum at every minor release.
>
> This is a rolling tracking doc. Per-item acceptance detail lives in the individual PRDs and in [BACKLOG.md](BACKLOG.md) / [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Use this doc to see *where we are across the whole roadmap* at a glance.

## Legend

| Tier | Meaning |
|------|---------|
| ✅ Complete | Acceptance criteria met; no material gaps. Maintenance-only. |
| 🟢 Substantially complete | Core shipped; remaining gaps are scoped follow-ups tracked in BACKLOG.md. |
| 🟡 Partial | Meaningful work shipped but major sections missing or unwired. |
| 🟠 Scaffolded | Types/skeleton exist with little operational code. |
| 🔴 Not started | No meaningful code in tree. |
| ⚪ Spec-only / deferred | PRD written; implementation phased out of current scope. |

## Summary

| PRD | Title | Status | One-line state |
|-----|-------|--------|----------------|
| 01 | Kernel & Event System | ✅ | Event bus, lifecycle, capability system all live |
| 02 | Security Model | ✅ | WASM sandbox, capability gating, audit logging, install-time consent shipped |
| 03 | Storage Engine | ✅ | Forge layout + SQLite + Tantivy + graph + watcher + CRDT hooks |
| 04 | Plugin System | ✅ | Manifest, WASM, hot-reload, activation events, community/core tiers |
| 05 | CLI | 🟢 | 12 subcommand groups live; agent/workflow CLIs blocked on their subsystems |
| 06 | File Formats | ✅ | Markdown/MDX/Canvas/Bases/forge config all parse + serialize |
| 07 | Theming & UI | ✅ | 497-token CSS registry, theme core plugin, contribution registry |
| 08 | Editor Engine | 🟡 | 3.7k LoC block/transaction/undo core; PRD §4 `BlockPositionMap` model superseded by CM6-owns-text |
| 09 | Terminal & Process Manager | 🟢 | Phases A–V shipped; `nexus-terminal` crate has 239 tests, `com.nexus.terminal` core plugin exposes 10 dispatch handlers, **both** editor-shell surfaces (nexus-tui pane + Tauri React panel) render live PTY output through kernel IPC; remaining work is ANSI-colour render (xterm.js), saved-commands sidebar, and FTS5 scrollback index |
| 10 | Database Engine | 🟡 | `.bases` parse + validate + formula IPC + **view application engine** (filter/sort/group → `AppliedView` for all 4 view types over `com.nexus.database::apply_view`); UI renderers for Board/List/Calendar/Gallery in progress |
| 11 | Git Integration | 🟢 | 1.1k-line `GitEngine` over `git2`; worker-thread wrapper for UI still needed |
| 12 | AI Engine | 🟡 | Anthropic/OpenAI/Ollama providers + chunker; no chat UI, no streaming, no agents |
| 13 | Skills | ⚪ | Spec only; no parser, registry, or CLI |
| 14 | MCP Integration | 🟡 | 807-line `serve_stdio`; no WebSocket/HTTP transports, no Host (client) |
| 15 | Agent System | ⚪ | Spec only; no Agent trait or planner |
| 16 | Workflow System | ⚪ | Spec only; no `.workflow.toml` parser or triggers |
| 17 | Cross-Platform Strategy | 🟢 | Tauri desktop shipping; web OPFS + mobile UniFFI deferred |

## Per-PRD detail

### PRD-01 — Kernel & Event System ✅
**Shipped:** `NexusEvent` enum, `EventBus`, `EventFilter`, `Kernel` lifecycle, `KernelPluginContext`, KV store.
**Gaps:** None material. Kernel `start`/`shutdown` docstrings updated to match reality (BACKLOG_COMPLETED F-1.1.1).
**Evidence:** [crates/nexus-kernel/src/{event,event_bus,kernel,context}.rs](crates/nexus-kernel/src/).

### PRD-02 — Security Model ✅
**Shipped:** `Capability` + risk classification, capability gating on IPC / fs / net / events / notify, path-traversal + TOCTOU fixes, audit log helpers, install-time HIGH-risk consent via `granted_caps.json` (F-5.1.1), epoch-deadline execution timeout, fuel-per-call reset.
**Gaps:** F-8.1.1 iframe sandbox for JS plugins (red; multi-week); F-8.1.2 boundary-bound `pluginId` (red; blocked on F-8.1.1).
**Evidence:** [crates/nexus-security/src/](crates/nexus-security/src/), [crates/nexus-plugins/src/loader.rs](crates/nexus-plugins/src/loader.rs) (`build_capabilities`, grant/revoke).

### PRD-03 — Storage Engine ✅
**Shipped:** Forge init, SQLite (files/blocks/links/tags/properties), Tantivy FTS, atomic writes, notify-debouncer watcher, kernel-bus event forwarding, petgraph knowledge graph with backlinks + unresolved-link tracking, CRDT state persistence.
**Gaps:** `BL-003` scoping operators (`tag:`, `path:`, `prop:`, `type:`) parse but post-filter incomplete; `BL-004` 3-tier link resolution cascade; `BL-006` block-level RAG chunk context prefix.
**Evidence:** [crates/nexus-storage/src/{schema,search,graph,watcher}.rs](crates/nexus-storage/src/).

### PRD-04 — Plugin System ✅
**Shipped:** Manifest parsing + validation, wasmtime sandbox, dual-tier loader (community + core), `CompositeIpcDispatcher` fall-through, hot-reload with retry/rollback, reentrancy detection, crash-quarantine counter, `--safe-mode` flag, activation events, script (JS) runtime with `onInit/onStart/onStop`, deterministic shutdown order.
**Gaps:** `nexus-plugin-api` Rust crate extraction still open (F-2.1.1); TypeScript `@nexus/extension-api` npm package already shipped.
**Evidence:** [crates/nexus-plugins/src/](crates/nexus-plugins/src/), [app/src/plugins/scriptRuntime.ts](app/src/plugins/scriptRuntime.ts).

### PRD-05 — CLI 🟢
**Shipped:** 12 command groups (`forge`, `content`, `plugin`, `ai`, `bases`, `canvas`, `config`, `git`, `graph`, `logs`, `mcp`, `watch`), output formatters (json/jsonl/text/table), structured exit codes, external-subcommand dispatch to plugins.
**Gaps:** `BL-001` daily notes; agent/workflow/skill runners blocked on their respective subsystems.
**Evidence:** [crates/nexus-cli/src/commands/](crates/nexus-cli/src/commands/).

### PRD-06 — File Formats ✅
**Shipped:** CommonMark + GFM (tables, task lists, strikethrough), wikilinks, embeds, block refs, tags, callouts, math, footnotes; YAML frontmatter reserved keys + plugin-extensible custom fields; MDX JSX extractor; Canvas JSON (Obsidian-compatible); `.bases` TOML + external records.
**Gaps:** None material.
**Evidence:** [crates/nexus-formats/src/](crates/nexus-formats/src/), [crates/nexus-storage/src/{mdx,canvas}.rs](crates/nexus-storage/src/).

### PRD-07 — Theming & UI ✅
**Shipped:** 497-variable CSS registry across 10 tiers, `ThemeCorePlugin` (`com.nexus.theme`) with 11 IPC handlers, hot-reload via kernel-bus → Tauri event forwarder, contribution registry for plugin-contributed commands/panels/menus/keybindings/snippets/tree providers/file handlers/URI handlers/webview panels, 14+ core components, workspace layout with drag-to-reorder/drag-to-split/persist.
**Gaps:** Platform chrome (macOS vibrancy, Windows Mica) is CSS-variable stubs only — native rendering not wired. Touch gestures in §12.3 not implemented.
**Evidence:** [crates/nexus-theme/src/core_plugin.rs](crates/nexus-theme/src/core_plugin.rs), [app/src/contributions/registry.ts](app/src/contributions/registry.ts).

### PRD-08 — Editor Engine 🟡
**Shipped:** 3,718 LoC block-tree core — `Block`, `BlockType`, transactions (insert/delete/merge/split), undo tree, annotations, `EditorCorePlugin`; CodeMirror 6 surface with syntax highlighting, keybindings, decoration compartments, snippet compartment, 800 ms debounced `editor_sync_content` IPC. **PRD-08 §4 has been amended** (commit 6f3b36d) to document CM6-owns-text + debounced-sync as canonical and retire the original `BlockPositionMap` spec — plugin authors reading §4.1/§4.4 now see the actual architecture. **PRD-08 §7 MDX Component Runtime now shipping** — `contributions.registerMdxComponent`, `ctx.editor.registerMdxComponent`, a CM6 `ViewPlugin` that scans visible text for `<Name prop="val" />` tags and renders each as a declarative-`PanelNode` widget via a host-owned DOM dispatcher (no `@mdx-js/mdx`, no `'unsafe-eval'`, CSP-safe). Built-in components `Card`, `Callout`, `Alert`, `Badge` registered in `builtins.ts`; hot-reload re-configures the compartment on any register/dispose.
**Gaps:**
- MDX block-form components (`<Card>…</Card>` with nested markdown children) — v1 supports self-closing only.
- Database view blocks (`[[{db:query}]]`) — no query executor or grid renderer.
- Inline AI edit suggestions — tool hooks exist, no UI.
**Evidence:** [crates/nexus-editor/src/](crates/nexus-editor/src/), [app/src/components/surfaces/EditorSurface.tsx](app/src/components/surfaces/EditorSurface.tsx), [app/src/editor/mdxComponentExtension.ts](app/src/editor/mdxComponentExtension.ts), [app/src/contributions/builtins.ts](app/src/contributions/builtins.ts), [docs/PRDs/08-editor-engine.md §4.1/§4.4/§7](docs/PRDs/08-editor-engine.md).

### PRD-09 — Terminal & Process Manager 🟢
**Shipped (Phases A–L, library foundation — §1/§2.1/§3/§4/§5.1/§5.2/§6/§8/§9):** Phase A–L shipped as described in earlier revisions of this doc: [`Session`](crates/nexus-terminal/src/session.rs) over `portable-pty`, [`OutputBuffer`](crates/nexus-terminal/src/buffer.rs) byte ring (§3.1/§3.4), [`SessionManager`](crates/nexus-terminal/src/manager.rs) with 50-session cap, signal ladder + Unix process-group kill (§5.1/§5.2), [`strip_ansi`](crates/nexus-terminal/src/ansi.rs) + [`LineBuffer`](crates/nexus-terminal/src/lines.rs) (§3.2/§3.3), [`nexus term`](crates/nexus-cli/src/commands/term.rs) CLI, [`detect_urls`](crates/nexus-terminal/src/urls.rs) (§6), env resolution + `.env` + interpolation + secret masking (§8), `ProcessState` (§4.2), and the compound-chain [parser + executor](crates/nexus-terminal/src/compound.rs) (§9).
**Shipped (Phase M — §1.3/§2.2/§2.3/§4/§5.3/§10.1, commit [516d484](https://github.com/baileyrd/nexus/commit/516d484)):** [`SqliteSessionStore`](crates/nexus-terminal/src/persist.rs) persists session metadata + on-disk scrollback blobs at `{scrollback_dir}/{id}/scrollback.bin`; [`SessionManager::evict_lru`](crates/nexus-terminal/src/manager.rs) / `spawn_or_evict` replace the hard-cap error with LRU eviction that returns the evicted scrollback snapshot for the caller to persist. [`SqliteAdHocStore`](crates/nexus-terminal/src/adhoc.rs) implements §10.1 ad-hoc history with `(command, working_dir)` dedupe + run-count increment + `AdHocStatus` tagging. [`ManagedProcess`](crates/nexus-terminal/src/procmgr.rs) encodes the richer §4.1 FSM (`Stopped`/`PreCommand`/`Starting`/`Running`/`Crashed`/`Restarting`) with backoff schedule + restart-attempt accounting; layered on top of `ProcessState`, not a replacement. [`profile_source_command`](crates/nexus-terminal/src/profile.rs) emits guarded rc-file sourcing for bash/sh/zsh/fish (§1.3); no-op for cmd/pwsh. [`JobObject`](crates/nexus-terminal/src/job_object.rs) wraps Win32 Job Objects with `KILL_ON_JOB_CLOSE` + `BREAKAWAY_OK` for §5.3; Unix builds get a stub type that documents the process-group path as the equivalent.
**Shipped (Phase N — §11 programmable terminal API, commit [d585578](https://github.com/baileyrd/nexus/commit/d585578)):** Sessions in `SessionManager` now carry a `LineBuffer` + name + `created_at`; new `set_name` / `line_count` / `lines_snapshot` / `lines_search` helpers. [`TerminalServer`](crates/nexus-terminal/src/server.rs) trait covers `create_session` / `close_session` / `send_input` / `send_raw_input` / `pump` / `read_output` / `search_output` / `subscribe_events` / `wait_for_pattern` / `get_session_info` / `list_sessions`. [`InMemoryTerminalServer`](crates/nexus-terminal/src/server.rs) default impl uses a `std::sync::mpsc` event bus, prunes dropped subscribers on every emit, and tracks per-session emitted-line high-water-marks so late subscribers don't replay history. Kept sync + runtime-agnostic.
**Shipped (Phase O — §10.2/§13/§15 saved commands + ad-hoc promotion, commit [e02ee25](https://github.com/baileyrd/nexus/commit/e02ee25)):** [`SavedCommand`](crates/nexus-terminal/src/saved.rs) struct mirroring the PRD §13 schema; [`SqliteSavedCommandStore`](crates/nexus-terminal/src/saved.rs) with `create` / `update` / `get` / `list` / `reorder` / `delete`; `list()` orders by `sidebar_order` (nulls last) + `name`. `slugify()` lowercases + collapses non-alphanumerics with a fallback for empty inputs. `promote_adhoc_to_saved` reads an `AdHocRecord`, builds a `SavedCommand` with explicit slug / icon / shell overrides, persists it, and returns the row for the sidebar to render without a round-trip.
**Shipped (Phase P — `com.nexus.terminal` core plugin, commit [0ecc173](https://github.com/baileyrd/nexus/commit/0ecc173)):** [`TerminalCorePlugin`](crates/nexus-terminal/src/core_plugin.rs) wraps `InMemoryTerminalServer` behind a `Mutex` to satisfy `CorePlugin: Send + Sync`. Exposes 10 append-only handler ids: `create_session` (1) through `list_sessions` (10). `subscribe_events` deliberately deferred — needs a plugin-host stream convention. Saved-commands / ad-hoc CRUD will be a sibling plugin in a later slice to keep handler surfaces small.
**Shipped (Phase Q — §4.4 pre-command runner, commit [a5053c1](https://github.com/baileyrd/nexus/commit/a5053c1)):** [`run_pre_commands`](crates/nexus-terminal/src/precmd.rs) drives a `ManagedProcess` through its `pre_commands` chain against a `TerminalServer` session. Sentinel-based exit detection: each step is wrapped with `; printf '<sentinel> %d\n' $?` so state (`cd`/`source`/`export`) inherits in the live shell while the exit code is still recoverable. Handler filters matching sentinel lines to integer-tailed ones, skipping the shell's input echo. `PreCommandOutcome` (`AllSucceeded`/`Skipped`/`StepFailed`/`StepTimedOut`) routes each branch into a different FSM transition. POSIX-only; cmd.exe / pwsh is a follow-up.
**Shipped (Phase R — §7 memory monitoring, commit [a67ee45](https://github.com/baileyrd/nexus/commit/a67ee45)):** [`read_process_rss`](crates/nexus-terminal/src/memory.rs) reads RSS via `/proc/<pid>/status` VmRSS on Unix, `GetProcessMemoryInfo().WorkingSetSize` on Windows. `MemoryLimits { soft_mb, hard_mb }` with `default_recommended` (250/500 MB per PRD §7.3). `MemoryLimitAction` returns `Ok` / `SoftExceeded` / `HardExceeded`; hard beats soft when both tripped. `MemoryMonitor` keeps a 60-sample rolling history per pid (`track` / `untrack` / `sample` / `set_limits`); policy/mechanism split means `HardExceeded` is returned to the caller, killing is the session layer's job.
**Shipped (Phase S — §12 AI suggestion engine, commit [040c4fe](https://github.com/baileyrd/nexus/commit/040c4fe)):** [`AiSuggestionEngine`](crates/nexus-terminal/src/ai.rs) observes output lines and emits `SuggestedCommand`s. Five built-in rules: `cargo.compile_failure` → `cargo check --message-format=json`; `npm.package_not_found` → `npm update`; `shell.command_not_found` → `which <name>` (bash + zsh formats); `git.permission_denied_publickey` → `ssh-add -l`; `net.address_in_use` → `lsof -iTCP -sTCP:LISTEN -nP`. `SuggestionRule` is a small trait so rules can cache compiled regex state; LLM bridging can layer on the same `SuggestedCommand` surface.
**Shipped (Phase T — §17 Criterion benches, commit [7e5428c](https://github.com/baileyrd/nexus/commit/7e5428c)):** `benches/buffers.rs` covers 100k-line ingest throughput + 10k-line buffer footprint; `benches/lines.rs` covers literal + regex search over a 100k-line buffer. `--quick`-friendly for CI.
**Shipped (Phase U — TUI editor-shell integration, commits [7f15e8d](https://github.com/baileyrd/nexus/commit/7f15e8d), [ceb2dc9](https://github.com/baileyrd/nexus/commit/ceb2dc9)):** Bootstrap registers `com.nexus.terminal` in the manifest with all 10 handler ids. [`nexus_bootstrap::terminal`](crates/nexus-bootstrap/src/terminal.rs) typed IPC helper mirrors `storage` / `database` — every call goes through `ctx.ipc_call("com.nexus.terminal", …)`, no direct `nexus-terminal` dep in the TUI crate. [`nexus-tui`](crates/nexus-tui/src/app.rs) adds `Mode::Terminal`, `TerminalPanelState`, an event-loop pump that runs each frame, and a [terminal pane](crates/nexus-tui/src/ui/terminal.rs) that wins over the task view; `T` opens, `Esc` hides, `Ctrl+C` delivers SIGINT via raw input (0x03), `Ctrl+D` kills. Status bar gains a magenta `TERM` badge. Validated end-to-end against `/bin/zsh` under WSL Fedora.
**Shipped (Phase U polish — PTY reader thread, commit [ceb2dc9](https://github.com/baileyrd/nexus/commit/ceb2dc9)):** `Session::read` moved onto a per-session background thread that does the blocking `reader.read` and forwards chunks through a `std::sync::mpsc` channel. The old WouldBlock-polling loop assumed `portable-pty` returned `WouldBlock` on idle; on Linux it's fully blocking, so an idle shell hung the TUI for 30 s (the IPC timeout). The reader-thread design honours `recv_timeout` correctly and exits naturally on EOF when the child is killed.
**Shipped (Phase V — Tauri desktop surface, commit [da2ac11](https://github.com/baileyrd/nexus/commit/da2ac11)):** 9 `tauri::command`s in [`nexus-app/src/terminal.rs`](crates/nexus-app/src/terminal.rs) adapting `term_create_session` / `_close_session` / `_send_input` / `_send_raw_input` / `_pump` / `_read_output` / `_search_output` / `_get_session_info` / `_list_sessions` to `ctx.ipc_call("com.nexus.terminal", …)`. Typed TS wrappers in [`app/src/ipc/terminal.ts`](app/src/ipc/terminal.ts) mirror the Rust command surface. [`TerminalPanel.tsx`](app/src/components/panels/TerminalPanel.tsx) React component spawns a session on mount, pumps every 120 ms to stream output, line-buffers user input (Enter → send_input; Ctrl+C → raw 0x03; Ctrl+D → close), auto-scrolls to bottom. New [`openContentTab`](app/src/stores/layout.ts) action on the layout store opens non-file tabs by content-type. Registered in [`contributions/builtins.ts`](app/src/contributions/builtins.ts) as content-type `"terminal"` + `workspace.open-terminal` palette command bound to `Mod+Shift+T`. Validated end-to-end against zsh under WSL Fedora.
**Gaps:** No ANSI-colour rendering (lines are ANSI-stripped `OutputLine.content`; full terminfo pass-through via xterm.js is a follow-up). No saved-commands sidebar in either surface. No FTS5 scrollback index (§19.3). No `nexus proc` CLI. Saved-commands / ad-hoc CRUD isn't exposed through a core plugin yet (library only). No event streaming over plugin IPC (in-process `std::sync::mpsc` only). No `subscribe_events` dispatch handler pending a plugin-host stream convention.
**Architecture posture:** Every subsystem in this PRD is a plain library except `TerminalCorePlugin`, which is the single integration point with the microkernel. **Both** editor-shell surfaces (nexus-tui ratatui pane, Tauri React panel) reach the terminal exclusively through `ipc_call("com.nexus.terminal", …)`; neither links `nexus-terminal` directly. Matches ARCHITECTURE §7 invariant #3.
**Evidence:** [crates/nexus-terminal/](crates/nexus-terminal/) (239 unit tests), [crates/nexus-bootstrap/src/{lib,terminal}.rs](crates/nexus-bootstrap/src/), [crates/nexus-tui/src/{app,input,ui/terminal}.rs](crates/nexus-tui/src/), [crates/nexus-app/src/terminal.rs](crates/nexus-app/src/terminal.rs), [app/src/{ipc/terminal.ts,components/panels/TerminalPanel.tsx,contributions/builtins.ts}](app/src/).

### PRD-10 — Database Engine 🟡
**Shipped:** `.bases` TOML schema + validation (3.4k LoC), property types (Title/Select/Date/Number/MultiSelect/People/timestamps), relations, CSV import/export, formula evaluator behind `com.nexus.database` IPC (`formula_eval` handler).
**Shipped (view engine, commit [a4c1bcb](https://github.com/baileyrd/nexus/commit/a4c1bcb)):** [`views.rs`](crates/nexus-database/src/views.rs) — pure-logic `apply_view(records, schema, view) -> AppliedView` pipeline. Supports 14 filter operators (`eq` / `!=` / `gt` / `gte` / `lt` / `lte` / `contains` / `icontains` / `starts_with` / `ends_with` / `is_empty` / `is_not_empty` / `in`); multi-level sort with null-last semantics in both directions; Kanban grouping by `group_field` with a `(none)` sentinel bucket for missing keys; Calendar grouping by ISO-date prefix of the `date_field`. `AppliedView.layout` is `Flat { records }` for Table/Gallery, `Grouped { groups }` for Kanban/Calendar. Exposed as `com.nexus.database` handler `apply_view` (id 4); registered in bootstrap. 18 view-specific tests (120 total in the crate).
**Gaps:** UI renderers for Board/Kanban/List/Calendar/Gallery not yet shipped — the engine returns the right shape, the React components to render it are in progress. No `nexus db` CLI. No cross-database relation queries (rollup / lookup resolvers go through `com.nexus.storage`). `BL-002` typed property columns not implemented.
**Evidence:** [crates/nexus-database/src/{views,core_plugin}.rs](crates/nexus-database/src/), [crates/nexus-storage/src/bases/](crates/nexus-storage/src/bases/).

### PRD-11 — Git Integration 🟢
**Shipped:** 1,111-line `GitEngine` over `git2::Repository` (27 public methods covering open/status/diff/stage/commit/log/branch), 243-line `AutoCommitter`, `GitState` / `FileStatus` / `HunkDiff` types.
**Gaps:** `GitEngine` is not `Send`/`Sync` (documented — `git2::Repository` constraint). No worker-thread wrapper for UI-driven async ops. `nexus git` CLI absent. No git events emitted to the kernel bus. No merge/rebase conflict resolution UI.
**Evidence:** [crates/nexus-git/src/engine.rs](crates/nexus-git/src/engine.rs), [crates/nexus-git/src/auto_commit.rs](crates/nexus-git/src/auto_commit.rs).

### PRD-12 — AI Engine 🟡
**Shipped:** `AiProvider` trait + `chat()` impls for Anthropic, OpenAI, Ollama; `AiCorePlugin` registered as `com.nexus.ai`; RAG chunker with block-aware boundary detection + tests; `ChunkEmbedding` / `ChunkMatch` types; config-based provider detection.
**Gaps:** No chat UI. No streaming response handling. No inline completion. No tool registration for agents. No token budgeting. No embedding backend (vectorstore traits only). No PII/secret filters before egress.
**Evidence:** [crates/nexus-ai/src/{provider,anthropic,openai,ollama,chunker,rag}.rs](crates/nexus-ai/src/).

### PRD-13 — Skills ⚪
**Shipped:** Spec document only.
**Gaps:** No `.skill.md` parser, no registry, no activation, no composition, no CLI, no built-in skill library.
**Evidence:** N/A.

### PRD-14 — MCP Integration 🟡
**Shipped:**
- Server: 807-line `NexusMcpServer::serve_stdio` exposing Nexus forge ops to external AI clients over stdio.
- **Host / client (new):** [`McpClient`](crates/nexus-mcp/src/client.rs) spawns external MCP servers (filesystem, github, etc.) as child processes, runs the MCP initialize handshake behind a 15s timeout, and exposes `list_tools` / `list_resources` / `list_prompts` / `call_tool` over rmcp's `TokioChildProcess` transport. [`McpHostConfig`](crates/nexus-mcp/src/config.rs) parses `<forge>/.forge/mcp.toml` (Claude Desktop-style `mcp.json` analogue) into `McpServerSpec { command, args, env, disabled }`.
**Gaps:** No MCP Host **orchestrator** yet — nothing spawns `McpClient`s from `mcp.toml` at forge boot, keeps them alive, or exposes their tools to callers (AI engine, plugin IPC). No WebSocket / HTTP+SSE transport. No reconnection / pool management. No `nexus mcp connect` / `nexus mcp call` CLI.
**Evidence:** [crates/nexus-mcp/src/{server,client,config}.rs](crates/nexus-mcp/src/), [crates/nexus-cli/src/commands/mcp.rs](crates/nexus-cli/src/commands/mcp.rs).

### PRD-15 — Agent System ⚪
**Shipped:** Spec document only.
**Gaps:** No `Agent` trait, archetype impls, planner, plan executor, observation loop, memory persistence, user-approval flow, CLI, or UI.
**Evidence:** N/A.

### PRD-16 — Workflow System ⚪
**Shipped:** Spec document only.
**Gaps:** No `.workflow.toml` parser, trigger engine (cron/fs/db/git/webhooks), condition evaluator, action executor, step orchestrator, variable system, CLI, or template library.
**Evidence:** N/A.

### PRD-17 — Cross-Platform Strategy 🟢
**Shipped:** Tauri 2.x desktop shell with React/Zustand frontend, strict CSP, sandboxed webview panels, Rust core platform-agnostic, deep-link scheme dispatch plumbing.
**Gaps:** Web target — no OPFS read/write, no IndexedDB vector store, no service-worker sync. Mobile — no UniFFI Kotlin/Swift FFI, no iOS/Android shell. Platform chrome (vibrancy/Mica) CSS-only. Multi-window (detachable panels) not wired. Tauri updater signature verification deferred.
**Evidence:** [crates/nexus-app/](crates/nexus-app/), [app/src/](app/src/), [packages/nexus-extension-api/](packages/nexus-extension-api/).

## Cross-cutting observations

1. **Microkernel + plugin system is the strongest pillar.** PRDs 01/02/04 all ✅ with extensive BACKLOG_COMPLETED.md evidence. The contribution registry pattern scales across every UI extension point.
2. **Knowledge-graph stack is shipping-grade.** PRDs 03/06/07 complete; users can read, write, search, link, and theme notes end-to-end.
3. **Editor §4 doc-vs-code drift is the biggest technical-debt flag.** PRD-08 §4 describes an architecture that was not built. Amend the PRD before 1.0 GA or plugin authors will waste time.
4. **Terminal (09) moved from 🟠 to 🟢 in a single focused push** (phases M–V). Library surface is feature-complete against the PRD acceptance criteria (239 unit tests), the `com.nexus.terminal` core plugin is wired end-to-end, and **both** editor-shell surfaces (nexus-tui + Tauri app) render live PTY output through kernel IPC. Remaining follow-ups are cosmetic (ANSI colour render, saved-commands sidebar) — not blocking Agent/Workflow, which can drive processes through `ipc_call` today.
5. **AI (12) is 60% there, but the last 40% (chat + streaming + inline completion) is where users feel value.** Highest-leverage next investment if "AI-powered" is a positioning pillar.
6. **Skills / Agents / Workflows (13/15/16) are aspirational.** Specs are good; code is zero. Treat as Phase 2/3, not 1.0 scope.

## Risk hotspots

| Risk | Why it matters | Mitigation |
|------|----------------|------------|
| MCP Host absence | Positioned as "MCP-integrated" but can't consume external MCP servers | ✅ Addressed: `McpClient` + `McpHostConfig` in `nexus-mcp` spawn external servers from `mcp.toml` and expose `list_tools` / `call_tool` over rmcp's stdio transport. Host orchestrator (lifecycle manager that keeps clients live across the app) is the next follow-up. |
| Git `!Send` constraint | UI-driven git ops will block the main thread | ✅ Addressed: `GitWorker` + `GitWorkerHandle` in `nexus-git` moves the `git2::Repository` to a dedicated OS thread behind a request/response channel. |
| F-8.1.1 iframe sandbox deferred | Cannot ship community JS plugin marketplace safely | Policy recorded: script plugins are first-party-only until F-8.1.1 + F-2.2.1 land |
| Database views absent | `.bases` files load but render nothing useful | Scope views into a Phase-2 PRD-10b rather than shipping 10 half-done |

## How to keep this doc honest

- When a BACKLOG.md item moves to BACKLOG_COMPLETED.md, check whether its PRD's status tier should bump.
- When a PRD's gaps list shrinks to zero, mark ✅ and note the commit that closed the last gap.
- When a new audit (`docs/UI-AUDIT.md`, `docs/MICROKERNEL-AUDIT.md`) discovers a finding, add it to the affected PRD's Gaps line with the finding id.
- Avoid re-describing the PRD here — link to it. This doc is the state-of-the-build, not a second copy of the spec.
