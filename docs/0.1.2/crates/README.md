# Per-Crate Analysis

> **As of:** 2026-05-25. One file per workspace member, each a full description + documentation grounded in the crate's source (`Cargo.toml`, `src/`, tests, and its `nexus-bootstrap` registration). For the one-row-per-crate summary table, see [`../crates.md`](../crates.md); this directory is the deep-dive companion to it.

Each per-crate doc follows the same template: **Overview** (narrative role + microkernel fit) → **Position in the dependency graph** → **Public API surface** (module-by-module) → **IPC handlers** (exact command/args/returns/capability tables) → **Capabilities** → **Settings/Config** → **Events** → **Internals & notable implementation details** → **Tests**.

All 35 workspace members in `crates/` are covered. (`shell/src-tauri/` and `packages/nexus-extension-api/` are TS/Tauri and live in [`../shell.md`](../shell.md).)

## Reading order

If you're new to the codebase, read down the dependency graph: the two leaves define the contract, the kernel routes, and everything else is a service plugin reachable only over IPC.

## Foundation — leaves of the dependency graph

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-types](nexus-types.md) | Shared types: activity timeline, forge-path validators, `.base(s)` formats, plugin-id constants | — |
| [nexus-plugin-api](nexus-plugin-api.md) | The kernel/plugin contract: `Capability` enum + `CapabilitySet`, `IpcDispatcher`/`IpcError`, `NexusEvent`, `TrustLevel` | — |
| [nexus-panic-log](nexus-panic-log.md) | Panic hook appending to `~/.nexus-shell/logs/panic.log` (1 MiB rotation) | — |
| [nexus-fuzz](nexus-fuzz.md) | BL-103 security fuzz targets (path validation, namespace, capability parse, manifest parse) + stable-Rust smoke runner | — |

## Kernel tier

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-kernel](nexus-kernel.md) | Microkernel core: event bus, `PluginContext`, capability gate, IPC convergence (`ipc_call`), audit/metrics. Depends only on the two leaves | — (routes only) |
| [nexus-kv](nexus-kv.md) | `SqliteKvStore` — on-disk impl of the kernel's `KvStore` trait (`kv.sqlite3`) | — |

## Plugin infrastructure & security

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-plugins](nexus-plugins.md) | Plugin runtime: WASM sandbox (wasmtime), loader, hot-reload, manifest signing (BL-099), granted-caps at-rest encryption (BL-101), the concrete IPC dispatcher + lifecycle registry | — (owns dispatcher) |
| [nexus-security](nexus-security.md) | Credential vault (keyring), audit-log query, TLS pinning (BL-102), capability risk table | `com.nexus.security` (7) |

## Storage, documents & collaboration

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-storage](nexus-storage.md) | Forge owner & file-as-truth: atomic writes, SQLite index, Tantivy FTS, watcher, knowledge graph, tree-sitter symbol index (BL-114) | `com.nexus.storage` (72) |
| [nexus-formats](nexus-formats.md) | Pure-logic format library: markdown (comrak), canvas, config structs, CSV, Notion zip import/export | `com.nexus.formats` (2) |
| [nexus-database](nexus-database.md) | "Bases": property types, formula engine, view pipeline, CSV — SQL delegated to storage | `com.nexus.database` (6) |
| [nexus-editor](nexus-editor.md) | Editor engine: block tree, annotations, self-reversing transactions, branching undo tree (PRD-08 §1-5) | `com.nexus.editor` (15) |
| [nexus-crdt](nexus-crdt.md) | Op-based CRDT (RGA for text) for collaborative editing; wired via bootstrap's `CrdtPublisher` (BL-074) | — |
| [nexus-comments](nexus-comments.md) | Side-margin comment threads anchored to `block_id`, JSON sidecars per file (BL-050) | `com.nexus.comments` (7) |

## AI & agents

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-ai-runtime](nexus-ai-runtime.md) | Shared AI/agent task scheduler + worker pool + event republisher (BL-134 / ADR 0028) | `com.nexus.ai.runtime` (9) |
| [nexus-ai](nexus-ai.md) | AI engine: provider abstraction (Anthropic/OpenAI/Ollama), embeddings, RAG, indexing daemon, optional local embeddings | `com.nexus.ai` (26) |
| [nexus-agent](nexus-agent.md) | Agent system: Plan/Step model, executor, tool registry, transcript FTS5 store (PRD-15) | `com.nexus.agent` (18) |
| [nexus-skills](nexus-skills.md) | `.skill.md` parser + registry; skills as prompt fragments for AI/agents (PRD-13) | `com.nexus.skills` (8) |
| [nexus-audio](nexus-audio.md) | STT + TTS provider abstraction; remote + optional local whisper (BL-117) | `com.nexus.audio` (3) |

## Protocol hosts & integration

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-mcp](nexus-mcp.md) | MCP host (connect to external servers) + server exposing 19 forge tools; stdio/Streamable-HTTP (BL-023) | `com.nexus.mcp.host` (12) + 19 tools |
| [nexus-lsp](nexus-lsp.md) | LSP host: spawns language servers, bridges JSON-RPC (Content-Length); contribution model (BL-113 / ADR 0027) | `com.nexus.lsp` (14) |
| [nexus-dap](nexus-dap.md) | DAP host: spawns debug adapters, bridges protocol; near-mirror of LSP | `com.nexus.dap` (21) |
| [nexus-acp](nexus-acp.md) | ACP (Agent Client Protocol) host + inbound server; newline-delimited JSON-RPC 2.0 | `com.nexus.acp` (8) |
| [nexus-remote](nexus-remote.md) | Remote-forge JSON-RPC server + client over stdio — headless kernel for SSH-driven forges (BL-140) | — |
| [nexus-collab](nexus-collab.md) | Live-collaboration WebSocket relay + bus bridge; topic-agnostic, ws:// only in Phase 1 (BL-143) | `com.nexus.collab` (4) |

## Services & dev tooling

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-git](nexus-git.md) | Git integration (libgit2): status/diff/blame/log/commit/branch, auto-commit, LFS | `com.nexus.git` (38) |
| [nexus-terminal](nexus-terminal.md) | Terminal & process manager: PTY, output capture, per-OS signals, scrollback FTS, session persistence (PRD-09) | `com.nexus.terminal` (29) |
| [nexus-templates](nexus-templates.md) | `.template.md` parser + `{{var}}` substitution engine with built-in date vars | `com.nexus.templates` (5) |
| [nexus-workflow](nexus-workflow.md) | Workflow automation: trigger/condition/action, cron + event triggers, actions via `ipc_call` (PRD-16) | `com.nexus.workflow` (12) |
| [nexus-linkpreview](nexus-linkpreview.md) | OG/Twitter-card fetch with SSRF guard, 5 s timeout, 512 KiB cap | `com.nexus.linkpreview` (1) |
| [nexus-notifications](nexus-notifications.md) | Multi-channel dispatcher: desktop, Discord, Telegram, SMTP; source→channel router + inbox (BL-133) | `com.nexus.notifications` (5) |
| [nexus-theme](nexus-theme.md) | Theming engine: ~430 CSS-variable defaults, 5-stage cascade, snippets, 11 bundled themes | `com.nexus.theme` (11) |

## Assembly & frontends

| Crate | Role | CorePlugin |
|-------|------|------------|
| [nexus-bootstrap](nexus-bootstrap.md) | Central assembly point: sole linker of every service crate, `build_cli_runtime`/`build_tui_runtime`, `cap_matrix.toml`, `SqliteAuditStore`, `CrdtPublisher`, IPC schema emit, the workspace's integration-test home | — (registers all 23) |
| [nexus-cli](nexus-cli.md) | Primary CLI binary `nexus`; all functionality over `ipc_call`; hosts `mcp/acp serve`, `serve --stdio`, `ai chat` REPL, `plugin scaffold`, TUI launch | — (IPC caller) |
| [nexus-tui](nexus-tui.md) | Terminal UI (ratatui/crossterm); consumes `build_tui_runtime`, routes through `ipc_call` | — (IPC caller) |

---

## Reconciliation notes vs. `crates.md`

The per-crate deep dive surfaced discrepancies with the existing summary table ([`../crates.md`](../crates.md)) worth correcting there:

1. **`nexus-ai-runtime` is a CorePlugin.** `crates.md` (line 17) marks it "Has CorePlugin impl? = No", but it defines `AiRuntimeCorePlugin` and registers `com.nexus.ai.runtime` with 9 IPC handlers (`crates/nexus-ai-runtime/src/core_plugin.rs`; registered in `nexus-bootstrap/src/plugins/mod.rs` *before* `ai`). The summary's "Crates registering a CorePlugin = 23" count is correct (verified: 23 `register()` calls, `ai_runtime` included) — it's only the per-row flag that is wrong. The "Crates with no nexus-* deps (leaves)" framing also no longer holds for `nexus-fuzz` as a true leaf in the CorePlugin sense.

2. **`nexus-bootstrap` external deps.** `crates.md` lists "tokio, anyhow, tracing, rusqlite, zip". `zip` is a **dev-dependency** (Notion-import fixtures) and `schemars` is dev-only behind the `ts-export` feature — neither is a runtime dep. Runtime deps also include `async-trait`, `thiserror`, `serde`/`serde_json`, `toml`, `chrono`.

3. **Handler counts** are now pinned per crate (see each doc's IPC table). Notably storage = 72, git = 38, terminal = 29, dap = 21, agent = 18, editor = 15, lsp = 14, mcp = 12 IPC + 19 MCP tools, workflow = 12, theme = 11.

## Cross-cutting findings (from the per-crate audits)

These recurred across crates and are documented in detail in the individual files:

- **Capabilities are gated at the kernel IPC boundary, not inside service crates.** Almost every service plugin (storage, git, editor, terminal, agent, skills, notifications, audio, etc.) performs **no in-crate capability check** — enforcement lives in `nexus-bootstrap`'s `cap_matrix.toml` applied at `ipc_call`. The per-handler "capability" columns in these docs therefore reflect the cap matrix / `ipc-handlers.md`, not source-level checks.
- **Several `lib.rs` doc headers are stale**, asserting "this is NOT (yet) a core plugin" in crates that now register one (`nexus-agent`, `nexus-skills`, `nexus-git`'s `worker.rs`, `nexus-collab`). Code is authoritative; the comments predate the IPC bridges.
- **`security.write` / `security.audit.write` and several outbound-`net` capabilities are defined but unenforced**; `protocol.host.contribute` / `process.spawn` gates on the LSP/DAP/ACP register+spawn paths are documented intent, not yet enforced (filed hardening follow-ups).
- **Open capability-laundering surface** in `nexus-workflow` (issue #77): a caller of `run`/`run_digest` transitively gains the workflow plugin's caps; `implied_caps` + `audit = true` are the current stopgap.
- **`#82` caveats** in `nexus-types` (`.bases` non-atomic save, `serde(flatten)` field shadowing, unvalidated filter operators, lossy TOML↔JSON) and **`#72`/`#78` path-safety/SSRF** guards recur in storage/linkpreview.
