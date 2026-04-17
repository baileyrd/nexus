# Nexus PRD Implementation Status

> **Snapshot date:** 2026-04-17
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
| 09 | Terminal & Process Manager | 🔴 | No PTY integration; zero process-manager code |
| 10 | Database Engine | 🟡 | `.bases` parse + validate + formula IPC; views (board/list/calendar/gallery) absent |
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
**Shipped:** 3,718 LoC block-tree core — `Block`, `BlockType`, transactions (insert/delete/merge/split), undo tree, annotations, `EditorCorePlugin`; CodeMirror 6 surface with syntax highlighting, keybindings, decoration compartments, snippet compartment, 800 ms debounced `editor_sync_content` IPC.
**Gaps:**
- PRD §4 specifies `BlockPositionMap` + per-keystroke decoration pipeline; actual architecture is CM6-owns-text + debounced sync. **PRD never formally amended** — plugin authors reading §4 will be misled.
- MDX component rendering (JSX component registry in editor) absent.
- Database view blocks (`[[{db:query}]]`) — no query executor or grid renderer.
- Inline AI edit suggestions — tool hooks exist, no UI.
**Evidence:** [crates/nexus-editor/src/](crates/nexus-editor/src/) (block, transaction, undo_tree, tree, annotation, core_plugin), [app/src/components/surfaces/EditorSurface.tsx](app/src/components/surfaces/EditorSurface.tsx).

### PRD-09 — Terminal & Process Manager 🔴
**Shipped:** Nothing. `nexus-tui` crate exists but is an alternate CLI frontend, not a PTY host.
**Gaps:** No `portable-pty` integration, no process spawn/attach, no ANSI parser, no signal handling, no `nexus proc` / `nexus term` CLI implementations, no MCP process tools, no process lifecycle events.
**Evidence:** `grep portable_pty crates/` returns zero hits.

### PRD-10 — Database Engine 🟡
**Shipped:** `.bases` TOML schema + validation (3.4k LoC), property types (Title/Select/Date/Number/MultiSelect/People/timestamps), relations, CSV import/export, formula evaluator behind `com.nexus.database` IPC (`formula_eval` handler).
**Gaps:** **No views.** Board/Kanban/List/Calendar/Gallery all absent (`grep BoardView|KanbanView|...` returns zero hits). No `nexus db` CLI. No cross-database relation queries. `BL-002` typed property columns not implemented.
**Evidence:** [crates/nexus-database/src/](crates/nexus-database/src/), [crates/nexus-storage/src/bases/](crates/nexus-storage/src/bases/).

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
**Shipped:** 807-line `NexusMcpServer::serve_stdio` in [crates/nexus-mcp/src/server.rs](crates/nexus-mcp/src/server.rs); stdio transport functional and exercised by the `nexus mcp` CLI subcommand.
**Gaps:** Public surface is intentionally narrow (`new` + `serve_stdio`). No WebSocket or HTTP+SSE transport. No MCP Host (client for connecting to external servers). No mcp.toml server-discovery config. No resource enumeration beyond what stdio exposes. No reconnection / pooling for external servers.
**Evidence:** [crates/nexus-mcp/src/server.rs](crates/nexus-mcp/src/server.rs), [crates/nexus-cli/src/commands/mcp.rs](crates/nexus-cli/src/commands/mcp.rs).

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
4. **Terminal (09) is the only fully unstarted subsystem.** Blocks Agent/Workflow roadmap items that need to run processes.
5. **AI (12) is 60% there, but the last 40% (chat + streaming + inline completion) is where users feel value.** Highest-leverage next investment if "AI-powered" is a positioning pillar.
6. **Skills / Agents / Workflows (13/15/16) are aspirational.** Specs are good; code is zero. Treat as Phase 2/3, not 1.0 scope.

## Risk hotspots

| Risk | Why it matters | Mitigation |
|------|----------------|------------|
| PRD-08 §4 doc drift | Plugin authors mislead by stale PRD | Amend §4 before 1.0 GA |
| MCP Host absence | Positioned as "MCP-integrated" but can't consume external MCP servers | Add minimal MCP client before any marketing claim |
| Git `!Send` constraint | UI-driven git ops will block the main thread | Wrap `GitEngine` in worker-thread; spec the pattern once, reuse |
| F-8.1.1 iframe sandbox deferred | Cannot ship community JS plugin marketplace safely | Policy recorded: script plugins are first-party-only until F-8.1.1 + F-2.2.1 land |
| Database views absent | `.bases` files load but render nothing useful | Scope views into a Phase-2 PRD-10b rather than shipping 10 half-done |

## How to keep this doc honest

- When a BACKLOG.md item moves to BACKLOG_COMPLETED.md, check whether its PRD's status tier should bump.
- When a PRD's gaps list shrinks to zero, mark ✅ and note the commit that closed the last gap.
- When a new audit (`docs/UI-AUDIT.md`, `docs/MICROKERNEL-AUDIT.md`) discovers a finding, add it to the affected PRD's Gaps line with the finding id.
- Avoid re-describing the PRD here — link to it. This doc is the state-of-the-build, not a second copy of the spec.
