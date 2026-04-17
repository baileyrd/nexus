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
| 09 | Terminal & Process Manager | 🟡 | Phases A–L shipped (`nexus-terminal` crate + `nexus term` CLI + URL detection + env resolution + `ProcessState` + compound chain parser **+ executor**); daemon-backed multi-session registry, AI, SQLite persistence still pending |
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
**Shipped:** 3,718 LoC block-tree core — `Block`, `BlockType`, transactions (insert/delete/merge/split), undo tree, annotations, `EditorCorePlugin`; CodeMirror 6 surface with syntax highlighting, keybindings, decoration compartments, snippet compartment, 800 ms debounced `editor_sync_content` IPC. **PRD-08 §4 has been amended** (commit 6f3b36d) to document CM6-owns-text + debounced-sync as canonical and retire the original `BlockPositionMap` spec — plugin authors reading §4.1/§4.4 now see the actual architecture. **PRD-08 §7 MDX Component Runtime now shipping** — `contributions.registerMdxComponent`, `ctx.editor.registerMdxComponent`, a CM6 `ViewPlugin` that scans visible text for `<Name prop="val" />` tags and renders each as a declarative-`PanelNode` widget via a host-owned DOM dispatcher (no `@mdx-js/mdx`, no `'unsafe-eval'`, CSP-safe). Built-in components `Card`, `Callout`, `Alert`, `Badge` registered in `builtins.ts`; hot-reload re-configures the compartment on any register/dispose.
**Gaps:**
- MDX block-form components (`<Card>…</Card>` with nested markdown children) — v1 supports self-closing only.
- Database view blocks (`[[{db:query}]]`) — no query executor or grid renderer.
- Inline AI edit suggestions — tool hooks exist, no UI.
**Evidence:** [crates/nexus-editor/src/](crates/nexus-editor/src/), [app/src/components/surfaces/EditorSurface.tsx](app/src/components/surfaces/EditorSurface.tsx), [app/src/editor/mdxComponentExtension.ts](app/src/editor/mdxComponentExtension.ts), [app/src/contributions/builtins.ts](app/src/contributions/builtins.ts), [docs/PRDs/08-editor-engine.md §4.1/§4.4/§7](docs/PRDs/08-editor-engine.md).

### PRD-09 — Terminal & Process Manager 🟠
**Shipped (Phase A — PTY primitive):** New [`nexus-terminal`](crates/nexus-terminal/) crate adds `Session::spawn` over `portable-pty` (native PTY alloc, shell detection, child spawn, blocking read/write with wall-clock timeout, resize, kill, `try_wait_exit`, Drop-safe cleanup). `detect_default_shell` honours `$SHELL` then platform fallbacks (`/bin/bash` → `/bin/zsh` → `/bin/sh` on Unix; `%ComSpec%` → `cmd.exe` on Windows).
**Shipped (Phase B — output ring buffer):** [`OutputBuffer`](crates/nexus-terminal/src/buffer.rs) — byte-level FIFO with 10 MB default capacity (PRD-09 §3.1), FIFO eviction with a cumulative drop counter (§3.4), `contains`/`find` byte-pattern search (§3.3), two-slice raw view, and `Session::read_into_buffer(out, timeout)` drain helper that wires the PTY master into the buffer end-to-end.
**Shipped (Phase C — session manager):** [`SessionManager`](crates/nexus-terminal/src/manager.rs) — in-memory registry of `(Session, OutputBuffer)` pairs keyed by `SessionId`. Enforces the PRD-spec 50-session cap (`DEFAULT_MAX_SESSIONS`), tracks `last_accessed` per session (LRU data surface without policy yet), and exposes `spawn` / `write` / `drain` / `resize` / `kill` / `remove` / `buffer_snapshot` / `reap_exited`. `remove` returns the final buffer for post-mortem inspection.
**Shipped (Phase D — signal escalation):** `Signal::{Int, Term, Kill}` enum + `Session::send_signal(Signal)` + `Session::request_shutdown(step_timeout)` implementing the PRD-09 §5.1 INT→TERM→KILL ladder via `libc::kill` on Unix; `SessionManager::request_shutdown` passthrough. Returns the signal that actually terminated the child so callers can log escalation level. Windows degrades INT/TERM to force-kill because `portable-pty` doesn't expose softer shutdown — documented inline.
**Shipped (Phase E — line-indexed view):** [`strip_ansi`](crates/nexus-terminal/src/ansi.rs) handles CSI (colours/cursor/erase/256-colour/TrueColor), OSC (BEL+ST terminators), 2-byte escapes, backspace (UTF-8-scalar-aware), and lone-CR rewinds. [`LineBuffer`](crates/nexus-terminal/src/lines.rs) ingests PTY bytes, buffers partial lines across reads, stitches pending-plus-new on the next newline, stores `Line { timestamp, raw, text_only, repeats }` records capped at 10 000 lines (PRD §3.1/§3.3), collapses adjacent exact-duplicate lines into `repeats` counters (§3.1 spinner-dedup), and exposes exact-substring `find` + `regex-lite`-backed `find_regex`. [`Session::read_into`](crates/nexus-terminal/src/session.rs) drains both byte ring + line view in one read pass.
**Shipped (Phase F — process-group kill §5.2):** `Session::send_signal` now targets `libc::kill(-pid, SIG)` on Unix, signalling the child's entire process group rather than just the shell. `portable-pty`'s child is a session leader (`setsid` before exec) so `pgid == pid`, and subprocesses it spawned (e.g. `sleep` in `sh -c 'sleep 30 & wait'`) receive the signal through the same group. Windows unchanged (no portable SIGINT equivalent).
**Shipped (Phase G — `nexus term` CLI):** [`nexus term env`](crates/nexus-cli/src/commands/term.rs) prints the detected default shell; [`nexus term run <cmd> [--timeout N]`](crates/nexus-cli/src/commands/term.rs) spawns `sh -c <cmd>` through `Session`, drains lines through `LineBuffer`, prints ANSI-stripped output, returns the child's exit code (124 on timeout, GNU `timeout` convention); [`nexus term shell`](crates/nexus-cli/src/commands/term.rs) attaches an interactive PTY shell with Ctrl-C graceful shutdown. Stubs `Proc(StubArgs)` / `Term(StubArgs)` in `main.rs` replaced with real clap subcommand tree.
**Shipped (Phase H — URL detection §6):** [`detect_urls`](crates/nexus-terminal/src/urls.rs) scans a text line for `https?://`, `file://`, bare `localhost:PORT`, and `127.0.0.1:PORT` and returns `UrlMatch { start, end, raw, resolved, kind }` records. Trailing sentence punctuation is stripped (`.`, `,`, `;`, `!`, `?`, `)`, `]`, `}`, `>`). `resolve_url` rewrites `localhost:NNN…` → `http://127.0.0.1:NNN…` (§6.2 loopback fix for dev-server URLs). Overlapping matches (`https://localhost:3000`) de-dup to the more specific HTTP kind. `Line::urls()` convenience on the line view. Regexes are compiled once via `OnceLock` — on-demand detection (§6.3 "incremental, not entire buffer") stays the caller's call.
**Shipped (Phase I — env resolution + `.env` parsing §8):** [`parse_env_file`](crates/nexus-terminal/src/env.rs) / `parse_env_text` return ordered `Vec<(key, value)>` (preserves declaration order for display and `export` semantics), stripping balanced single- or double-quoted values, skipping comments and blank lines, and surfacing duplicates so callers can see conflicts. [`resolve_env`](crates/nexus-terminal/src/env.rs) merges four PRD §8.1 layers (command → `.env` → shell → nexus-injected) with later writes winning on collision and first-sighting preserved. [`interpolate_env`](crates/nexus-terminal/src/env.rs) expands `$VAR` / `${VAR}` references across the merged set up to 10 passes (PRD §8.3), leaving unknown refs literal and surviving cycles. [`is_secret_key`](crates/nexus-terminal/src/env.rs) / [`mask_secrets`](crates/nexus-terminal/src/env.rs) mark values under `*API*`, `*KEY*`, `*SECRET*`, `*TOKEN*`, `*PASSWORD*` (case-insensitive) for redaction in the process panel and logs.
**Shipped (Phase J — process lifecycle state §4):** `ProcessState { Running, Exited { code }, Killed { signal, code } }` enum exposing `is_running` / `is_terminated` / `exit_code()` helpers. [`Session::state()`](crates/nexus-terminal/src/session.rs) returns the latched state — cheap, non-polling. Internal tracking: `send_signal` and `kill` record the signal; `try_wait_exit` latches the terminal state and attributes the exit to `Killed { signal }` if we asked, `Exited { code }` otherwise. [`SessionManager::state(id)`](crates/nexus-terminal/src/manager.rs) passthrough. The richer PRD §4 states (`Stopped`, `PreCommand`, `Restarting`) live on the future process-manager layer that wraps sessions with auto-restart + pre-command policy — out of scope for this pass.
**Shipped (Phase K — compound-command chain parser §9):** [`parse_command_chain`](crates/nexus-terminal/src/compound.rs) splits an input string into `Vec<CommandStep { operator, command }>` at top-level `&&` / `||` / `;` operators. Hand-rolled scanner treats single- and double-quoted regions as opaque — `echo "a && b"` stays one step, unlike a naive regex split. Empty steps from stray operators (`a ; ; b`) are dropped. `Operator::{And, Or, Seq}` + `CommandStep::should_run(previous_exit_code)` encode the per-step gating semantics exactly as PRD §9.1 specifies. [`requires_single_shell`](crates/nexus-terminal/src/compound.rs) detects `cd`/`pushd` in the chain (word-boundary-aware so `cdk-app` doesn't trigger) so callers know to pipe the chain into one long-lived shell instead of spawning per step (§9.3).
**Shipped (Phase L — chain executor §9.2):** [`execute_chain(steps, run_step)`](crates/nexus-terminal/src/compound.rs) walks a `[CommandStep]` in order, threading the previous step's exit code through each `should_run` gate and invoking the caller-supplied `run_step` closure to execute. Returns a `ChainOutcome { steps: Vec<(CommandStep, StepOutcome)>, final_exit_code }` where each `StepOutcome` is `Ran { exit_code } | Skipped { reason: SkipReason } | Failed { error }`, plus `all_ok()` / `ran_steps()` accessors. Runner errors are captured (not bubbled) and treated as non-zero exits so `&&` successors skip and `||` successors still get a chance to run, matching shell semantics. Runner-agnostic by design: tests inject synthetic exit codes; real callers wrap `Session::spawn` per step or pipe through a single long-lived shell. 141/141 unit tests pass + 1 ignored; +11 executor tests (empty chain, single step, `&&` skip, `||` skip, `;` always-runs, runner-error treated as non-zero, `all_ok` true/false/failure, `ran_steps` excludes skipped, `SkipReason` descriptions). Clippy `-D warnings` clean.
**Gaps (Phase M+):** No event emission (PRD §9.2 `ProcessStep{Starting,Completed,Skipped}` bus events — plugs into the core-plugin layer when it lands). No daemon-backed multi-session registry (name-addressed sessions surviving across CLI invocations — depends on §2.2 SQLite persistence + IPC socket). No Windows Job Objects (§5.3). No scrollback on disk / LRU eviction policy (§2.3). No process-manager layer (pre-commands + auto-restart with exponential backoff — PRD §4's `PreCommand`/`Crashed`/`Restarting` states — layered on top of `ProcessState`). No memory monitoring / global cap (§7, §3.4 pressure). No shell-profile sourcing on spawn (§1.3). No ad-hoc command system (§10). No programmable terminal API (§11). No AI integration (§12). No terminal UI components (§14). No `nexus proc` CLI (waits on §10 ad-hoc command system). Live stdin forwarding in `nexus term shell` (line-by-line display is enough for verification today).
**Architecture posture:** Phase A is an invoker-local library (same pattern as `nexus-git` and the MCP Host client). A future `com.nexus.terminal` core plugin can wrap `Session` in dispatch handlers for `com.nexus.terminal.spawn` / `write` / `read` / `kill` when the kernel gets a consumer; no redesign needed.
**Evidence:** [crates/nexus-terminal/src/{lib,session,shell,error}.rs](crates/nexus-terminal/src/), 12 passing tests including `spawn_read_echo_output_and_exit_cleanly` and `write_then_read_round_trip_through_cat`.

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
4. **Terminal (09) is the only fully unstarted subsystem.** Blocks Agent/Workflow roadmap items that need to run processes.
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
