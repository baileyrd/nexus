# Nexus vs. Tolaria — Architectural Comparison

A side-by-side look at two markdown-knowledge-base projects on this machine. They share almost identical *principles* (filesystem-as-truth, AI-native, local-first) but make very different *engineering* choices. Nexus is an extensible runtime platform; Tolaria is an opinionated end-user product.

## TL;DR

| | **Nexus** | **Tolaria** |
|---|---|---|
| One-line | Plugin-extensible knowledge **runtime** with CLI/TUI/desktop/MCP surfaces | Opinionated desktop **app** for managing a markdown vault, Bear-style |
| Primary language | Rust (≈90%) + TypeScript shell | TypeScript/React (frontend) + Rust (Tauri backend) |
| Architecture | **Microkernel + capability-gated IPC**; 20-crate Cargo workspace; WASM plugin sandbox | **Tauri-IPC client/server**; flat React app + scoped Rust command modules; no plugin system |
| Source of truth | Markdown files in a "Forge" + rebuildable SQLite/Tantivy index | Markdown files in a "Vault" + rebuildable JSON cache outside the vault |
| Search | Tantivy full-text index | `walkdir` keyword scan (no index) |
| Editor | CodeMirror 6 + custom block-tree editor | BlockNote (rich-text) + CodeMirror 6 (raw mode) |
| AI strategy | Multi-provider trait (Anthropic / OpenAI / Ollama / llama.cpp) with first-class RAG and an agent system | Spawns external **CLI agents** (Claude Code / Codex) — Tolaria itself stores no API keys |
| Versioning | None built-in | **Git-first** — every vault is a git repo, with auto-commit / sync / pulse view |
| Surfaces | `nexus` CLI, `nexus-tui` TUI, Tauri desktop shell, MCP stdio server | Tauri desktop app + bundled Node MCP server |
| MCP | First-class — both server (15 tools, all `nexus_*`-prefixed) **and** host (spawns external MCP servers) | First-class server (6 tools as actually shipped in `mcp-server/index.js`; the architecture doc lists ~14 aspirationally) over stdio + WebSocket bridges; not a host |
| Maturity | v0.1.0 alpha; ~80–100k LOC | v0.1.0; mature feature set, ~90 ADRs, public releases |

The shortest summary: **Nexus is a platform you build *on*; Tolaria is a product you use.**

---

## 1. Domain & purpose

Both projects index a directory of markdown files with YAML frontmatter and treat that directory as the single source of truth. From there they diverge.

**Nexus** is a *runtime*. The README calls it "a personal, plugin-extensible knowledge environment built in Rust" and the central design decision (ADR 0011) was to adopt a "plugin-first shell" where every visible UI element is a plugin contribution loaded by `ExtensionHost`. Beyond notes, Nexus ships a knowledge graph (petgraph), a Tantivy full-text index, a multi-provider AI subsystem with RAG, a WASM plugin sandbox, an agent/planner system, a workflow engine, a Bases (table) engine, a terminal/PTY manager, a skills library, and an MCP server *and* MCP host. It exposes itself through four invokers: the `nexus` CLI, a ratatui-based TUI, a Tauri desktop shell, and an MCP stdio server.

**Tolaria** is a *desktop app*. The README's first sentence: "Tolaria is a desktop app for Mac and Linux for managing markdown knowledge bases." The principles section emphasizes opinionatedness ("Convention over configuration"), keyboard-first power-user ergonomics (ADR 0020), and a tight feature loop driven by the author's daily use of a 10,000-note personal vault. The product is a polished four-panel UI (Sidebar / NoteList / Editor / Inspector-or-AI) inspired by Bear Notes, and the engineering investment shows up in details like crash-safe rename transactions (ADR 0075), concurrent-safe cache replacement (ADR 0077), Linux window-chrome handling (ADR 0079), AppImage WebKit workarounds, and dual-architecture macOS release artifacts (ADR 0083).

In short: Nexus optimizes for **extensibility and surface area**; Tolaria optimizes for **a finished, opinionated daily-driver experience**.

---

## 2. Tech stack

Both use Tauri 2 + Rust backend + a JS/TS frontend, but the *center of gravity* is in different places.

### Nexus

- **Cargo workspace, 20 crates.** The Rust code is the product. Crates include `nexus-kernel`, `nexus-storage`, `nexus-ai`, `nexus-plugins`, `nexus-cli`, `nexus-tui`, `nexus-mcp`, `nexus-security`, `nexus-bootstrap`, `nexus-database`, `nexus-git`, `nexus-terminal`, `nexus-theme`, `nexus-skills`, `nexus-agent`, `nexus-workflow`, `nexus-formats`, `nexus-kv`, `nexus-editor`, `nexus-linkpreview`.
- **Async runtime:** tokio v1.51.
- **Storage/index:** `rusqlite` + r2d2 pool, Tantivy v0.26 for FTS, petgraph v0.7 for the knowledge graph.
- **Plugin sandbox:** wasmtime v42, with capability-gated host functions.
- **AI:** `reqwest` for provider HTTP; provider trait with Anthropic/OpenAI/Ollama implementations.
- **Markdown:** `comrak` (CommonMark + GFM + custom extensions for wikilinks, embeds, callouts, MDX).
- **MCP:** `rmcp` v1.3 — both server and host.
- **Other Rust:** `notify` (file watching), `libgit2` (git), `portable-pty` (terminal), `clap` (CLI), `keyring` (OS credential vault), `ratatui` + `crossterm` (TUI).
- **Shell (`shell/`):** Tauri 2 + Vite + React 18 + CodeMirror 6 + xterm.js, packaged as a pnpm workspace with a public `@nexus/extension-api` package for plugin authors.

### Tolaria

- **Single TypeScript/React app + scoped Rust backend.** The TypeScript code is the product; Rust handles privileged operations and is structured as one binary with module-grouped commands (ADR 0030).
- **Frontend:** React **19**, TypeScript 5.9, Vite 7, Tailwind CSS v4, shadcn/Radix UI primitives.
- **Editor:** **BlockNote** v0.46 (rich-text WYSIWYG) for normal mode + CodeMirror 6 for raw markdown editing (ADR 0022, 0037, 0063).
- **Backend (Tauri Rust, edition 2021):** `gray_matter` for frontmatter, system `git` CLI invoked via `tokio::process` (no libgit2 — Tolaria deliberately avoids provider-specific OAuth, ADR 0056), `walkdir` for the keyword search.
- **MCP:** `mcp-server/` is a separate **Node.js** project using `@modelcontextprotocol/sdk` 1.0, spawned by Tauri at startup.
- **AI:** `@anthropic-ai/sdk` is bundled but the headline pattern is that Tolaria spawns **external CLI agents** (Claude Code, Codex) as subprocesses (ADR 0028, 0062). Tolaria itself does not store API keys.
- **Telemetry / app infra:** Sentry, PostHog (ADR 0016, 0042), `react-virtuoso`, `@dnd-kit`, KaTeX, Lara CLI for localization (ADR 0087), Husky + Playwright + Vitest for QA, CodeScene-managed code-health gates (ADR 0018, 0064).

### Where this lands you

- Nexus's surface for plugin authors is a stable **TypeScript API** (`@nexus/extension-api`) backed by a Rust kernel — closer in spirit to VS Code or Obsidian's plugin model.
- Tolaria has **no plugin system at all**. Extensibility comes through *vault conventions* (e.g. naming a frontmatter field `belongs_to:` triggers UI behavior automatically) and *external AI agents that touch the filesystem*. ADR 0028 ("CLI agent only, no API key") is an explicit design choice to keep Tolaria small and let the AI ecosystem evolve outside it.

---

## 3. Top-level layout

### Nexus

```
nexus/
├── Cargo.toml                # workspace manifest, 20 member crates
├── docs/
│   ├── ARCHITECTURE.md       # C4 model, mermaid diagrams, RAG flow, plugin FSM
│   ├── PRDs/                 # 17 product-requirement docs + IMPLEMENTATION_STATUS
│   └── adr/                  # architecture decision records
├── crates/                   # 20 Rust crates (kernel, storage, ai, plugins, cli, tui, mcp, …)
└── shell/                    # Tauri 2 + Vite + React desktop shell
    ├── src/                  # plugin host + first-party plugins
    ├── src-tauri/            # Rust shell crate (window setup, IPC bridge)
    └── packages/
        └── nexus-extension-api/   # public TS API for plugin authors
```

### Tolaria

```
tolaria/
├── package.json              # single pnpm project (frontend + tooling)
├── docs/
│   ├── ARCHITECTURE.md       # mermaid system diagrams, four-panel layout, data flow
│   ├── ABSTRACTIONS.md       # VaultEntry, types, semantic field names
│   └── adr/                  # ~90 ADRs, very actively maintained
├── src/                      # React frontend
│   ├── App.tsx               # orchestrator
│   ├── components/           # ~150 .tsx files (UI), each with co-located *.test.tsx
│   ├── hooks/                # vault loader, save, autocompletion, etc.
│   └── lib/                  # i18n catalogs, locales/
├── src-tauri/                # Tauri Rust backend
│   ├── src/
│   │   ├── lib.rs            # Tauri command registration
│   │   ├── vault/            # cache, scanner, rename transactions, parsing
│   │   ├── frontmatter/      # YAML ops
│   │   ├── git/              # commit, pull, push, history, conflict, pulse
│   │   ├── commands/         # split by domain (ADR 0030): vault, ai, git, system, …
│   │   ├── claude_cli.rs     # adapter for Claude Code subprocess
│   │   └── mcp.rs            # spawns Node MCP server, registers config
│   └── capabilities/         # Tauri permission scopes
├── mcp-server/               # standalone Node project (vault.js, ws-bridge.js, index.js)
├── design/                   # .pen design files for proposed features
├── demo-vault-v2/            # fixture vault used in development & tests
├── tests/smoke/              # Playwright smoke specs
└── scripts/                  # bundle-mcp-server.mjs, validate-locales.mjs, …
```

The structures themselves are a tell: Nexus segments by *subsystem* (each crate owns a capability), Tolaria segments by *layer* (frontend, backend, MCP).

---

## 4. Architecture

### Nexus: microkernel with strict IPC

Four invariants shape the codebase:

1. **File-as-truth.** Markdown on disk is authoritative; all indices (SQLite, Tantivy, knowledge graph) are rebuildable.
2. **Microkernel isolation.** `nexus-kernel` depends only on `nexus-types`. Every other crate depends on the kernel; the kernel never depends on subsystems.
3. **IPC uniformity.** Even the CLI/TUI talk to storage and AI through `PluginContext::ipc_call` — the same path community WASM plugins use. No direct linking.
4. **Capability gating.** Every kernel-mediated operation (`fs.read`, `kv.write`, `ipc.call`, `events.publish`, etc.) checks a capability before running.

The kernel exposes an async event bus and a typed IPC dispatcher. Storage, AI, security, terminal, agent, skills, workflow, MCP-host etc. all run as **core plugins** (native Rust, full access). Third parties can ship **community plugins** that compile to WASM and run inside `wasmtime` with capability-gated host functions. The plugin-first shell extends this same model into the UI: every panel, command, keybinding, file handler, and tree provider is a plugin contribution loaded by `ExtensionHost`.

This is a *real* extensibility story, with platform-grade concerns: hot-reload with reentrancy detection, crash quarantine, manifest validation, audit logging.

### Tolaria: Tauri client + scoped Rust commands

Tolaria's architecture is well-documented but conventional for a Tauri app:

- **Frontend (React):** `App.tsx` orchestrates. Hooks like `useVaultLoader`, `useEntryActions`, `useNoteActions`, `useEditorSave`, `useAutoSync`, `useCliAiAgent` mediate between React state and the backend. State management is deliberately propsy — ADR 0026 ("props down, no global state") is explicit about avoiding Redux/Zustand.
- **Backend (Rust):** A single Tauri binary registers commands. ADR 0030 split the formerly-monolithic command file into domain modules (`commands/vault/*`, `commands/git*`, `commands/ai`, `commands/system`, `commands/folders`, …). The vault module owns cache, scanner, rename transactions, and folder operations.
- **MCP server:** A separate Node project bundled at build time (`scripts/bundle-mcp-server.mjs`), spawned by Rust on app startup. Two WebSocket ports back the live UI ↔ MCP loop: 9710 for tool calls into the vault, 9711 for UI broadcasts back from MCP tools.
- **Three-form data invariant:** filesystem ↔ cache (`~/.laputa/cache/<hash>.json`) ↔ React `VaultEntry[]`. The architecture doc enforces five rules around this — e.g. "disk-first writes" (state must never update before the Tauri IPC call resolves) and "cache is disposable" (always reconstructible from disk).

The result is far less ceremony than Nexus, but also no plugin boundary: a contributor extending Tolaria edits the React app and the Rust commands directly.

### One concrete diff: how the AI panel calls a tool

- **Nexus.** The agent panel calls `ctx.ipc_call("com.nexus.ai", "stream_chat", { … })`. The kernel checks `Capability::IpcCall`, dispatches to the AI plugin, which streams tokens back through the event bus. The same path is taken by a community WASM plugin if it wants to call AI.
- **Tolaria.** `useCliAiAgent` invokes the Tauri command `stream_ai_agent`. Rust's `ai_agents.rs` picks an adapter (`claude_cli.rs` or the Codex JSON exec adapter), spawns the agent as a subprocess with `--sandbox workspace-write`, and emits a normalized `ai-agent-stream` event back. The agent talks to the vault via the bundled MCP server, not via Tolaria itself.

Both work; they just buy you very different things.

---

## 5. Capabilities

### Notes & content

| Capability | Nexus | Tolaria |
|---|---|---|
| Create / read / delete / list notes | ✓ (CLI + IPC) | ✓ (UI + Tauri commands) |
| Frontmatter parsing & editing | ✓ (`comrak` + custom) | ✓ (`gray_matter` + bespoke ops in `frontmatter/ops.rs`) |
| Wikilinks `[[…]]` | ✓ resolved + indexed | ✓ resolved + injected into BlockNote blocks; dynamic relationship detection per ADR 0010 |
| Embeds `![[…]]`, block refs `(uuid)`, callouts | ✓ | Partial — wikilinks yes, transclusion not the same focus |
| Daily notes | ✓ `nexus content daily` | Not as a first-class command |
| Tasks `[ ]` cross-file | ✓ `content tasks` / `task-toggle` | Lives inside individual notes; no cross-file task index |
| Knowledge graph (backlinks, neighbors, unresolved) | ✓ first-class (petgraph) | Backlinks via Inspector; no graph algorithms exposed |
| Math (KaTeX) | ✓ via custom markdown ext | ✓ first-class (ADR 0082) |
| Canvas (Obsidian-compatible) | ✓ `.canvas` parser | – |
| Bases / structured tables | ✓ TOML-backed table engine, kanban/calendar/gallery views | – (custom views via `.yml` saved-view files instead) |
| Saved views | – | ✓ `views/*.yml` with filter/sort/property columns |
| Git integration | ✓ via `libgit2`: status, diff, log, stage, commit, blame; auto-commit on forge changes | ✓ via system `git` CLI: clone, pull, push, commit, conflict resolution, **Pulse view** (commit feed UI), AutoGit idle/inactive checkpoints |

### AI

| | Nexus | Tolaria |
|---|---|---|
| Approach | Multi-provider trait in-process | External CLI agent subprocess |
| Providers | Anthropic, OpenAI, Ollama, llama.cpp | Claude Code, Codex (more agents pluggable per ADR 0062) |
| RAG | First-class — block-aware chunking, embeddings stored in SQLite, vector search via storage IPC | Not a built-in RAG pipeline; AI agents read files via MCP tools |
| Streaming | `stream_chat` / `stream_ask` token streams | Normalized `ai-agent-stream` events with TextDelta / ThinkingDelta / ToolStart / ToolDone / Done |
| Agents / planning | `nexus agent plan/run` with Writer/Coder/Researcher archetypes | The CLI agent itself does planning; Tolaria provides context, not a planner |
| Skills | `.skill.md` callable templates with parameters; built-in library | – |
| Workflows | TOML-defined workflows with cron triggers | – |
| API key handling | OS keyring (`keyring` crate) | None — agents authenticate themselves outside Tolaria (ADR 0028) |

### MCP

| | Nexus | Tolaria |
|---|---|---|
| Role | Server **and** host | Server only |
| Tools exposed | 13 — `nexus_read_note`, `nexus_create_note`, `nexus_update_note`, `nexus_delete_note`, `nexus_list_notes`, `nexus_search`, `nexus_backlinks`, `nexus_outgoing_links`, `nexus_graph_status`, `nexus_list_tags`, `nexus_list_tasks`, `nexus_toggle_task`, `nexus_ask` | 6 as shipped — `search_notes`, `get_vault_context`, `get_note`, `open_note`, `highlight_editor`, `refresh_vault` |
| Transport | stdio | stdio + WebSocket (9710 tools, 9711 UI bridge) |
| Notable | `nexus_ask` exposes the full RAG pipeline as a single MCP tool | `open_note` + `highlight_editor` let an MCP client drive the Tolaria UI directly via the WebSocket UI bridge |
| External-server hosting | ✓ — `McpClient` spawns external MCP servers; orchestrated by `com.nexus.mcp.host` core plugin | – |
| Setup with Claude/Cursor | Manual | Explicit in-app setup writes to `~/.claude.json`, `~/.claude/mcp.json`, `~/.cursor/mcp.json`, generic `~/.config/mcp/mcp.json` (ADR 0074) |

### Surfaces

| Surface | Nexus | Tolaria |
|---|---|---|
| CLI | `nexus` — full subcommand tree (forge/content/graph/plugin/ai/git/skill/agent/workflow/term/proc/mcp/tui/desktop) | – |
| TUI | `nexus-tui` (ratatui) | – |
| Desktop | Tauri shell, plugin-first | Tauri app, four-panel UI (Bear-style) |
| Multi-window | Per-plugin panels | Notes openable in dedicated `note-*` windows |
| Web preview | – | `pnpm dev` mock mode at `localhost:5173` for browser-based development |
| Localization | English | Lara CLI + JSON catalogs, multi-locale (ADR 0087) |
| Telemetry | – | Sentry + PostHog (with feature flags for canary releases — ADR 0042) |
| Auto-update | – | Tauri updater plugin |

---

## 6. Data model

### Nexus — relational + vector + graph

Indexes live in `.forge/`:

- `index.db` (SQLite) with tables: `files`, `blocks` (typed: paragraph/heading/codeblock/task/callout/embed/canvas/base), `links` (with resolved/unresolved state), `tags`, `properties`, `embeddings` (vector), `tasks`, `canvas`, `bases`.
- Tantivy FTS at `.forge/search/`.
- In-memory `petgraph` directed graph.
- `kv.sqlite3` for plugin settings.
- Append-only audit log at `.forge/logs/audit.log`.
- File formats: markdown (extended), `.canvas`, `.bases`, `.skill.md`, `.workflow.toml`, agent plans, chat sessions.

### Tolaria — single denormalized cache

- Vault is a flat directory of `.md` files at the root (ADR 0006); `type/` subfolder holds type-definition documents.
- One JSON cache file per vault, keyed by a hash of the vault path: `~/.laputa/cache/<vault-hash>.json` (kept *outside* the vault per ADR 0024). Contains `{ vault_path, git_head, version, entries: VaultEntry[] }`.
- The `VaultEntry` type is the entire data model: path, filename, title, `isA` (entity type), aliases, `belongsTo` / `relatedTo`, all-relationships map, outgoing wikilinks, status, timestamps, snippet, frontmatter properties, and `fileKind` (markdown / text / binary).
- `_field` underscore-prefixed frontmatter is the system-property convention (ADR 0008) — used by Type documents to set icon/color/order/sidebar-label/template.
- Cache invalidation is **git-driven**: incremental updates use `git diff old..new --name-only` to re-parse only changed files (ADR 0014). This is a clever co-design between the persistence model and the Git-first principle.
- Crash-safe rename transactions stage in a hidden `.tolaria-rename-txn/` folder inside the vault (ADR 0075).
- Per-vault UI state (zoom, view mode, layout, tag colors) lives in localStorage; per-vault list (path + label + active) lives at `~/.config/com.tolaria.app/vaults.json`.

The trade-off is stark: Nexus pays the engineering cost of multiple specialized indexes to make queries (FTS, graph, vector) fast on big knowledge bases; Tolaria leans on the filesystem and Git, then accepts a `walkdir` keyword scan for search (ADR 0009 explicitly defends "keyword-only search" as good enough).

---

## 7. External interfaces

**Nexus** is invokable from many directions:

- Headless CLI for scripting and CI.
- Interactive TUI for terminal users.
- Tauri desktop GUI.
- MCP stdio server for AI agents.
- Plugin contribution registry (commands, panels, menus, keybindings, snippets, tree providers, file handlers, URI handlers, webview panels) — third-party extension surface.
- IPC dispatch with capability gating, used uniformly by all surfaces.

**Tolaria** is one application with two side channels:

- The Tauri desktop window (with optional secondary note windows).
- The bundled MCP server, which other AI clients (Claude Code, Cursor) connect to.
- A WebSocket UI-action bridge so MCP tool calls can drive the Tolaria UI (highlight, open note, set filter).

---

## 8. Build, run, deploy

| | Nexus | Tolaria |
|---|---|---|
| Build (CLI/TUI) | `cargo build --release` | n/a |
| Build (desktop) | `cd shell && pnpm tauri:dev` (or `:build`) | `pnpm tauri dev` (or `tauri build`) |
| Browser dev | – | `pnpm dev` opens a mock-mode app at `localhost:5173` for fast iteration without Tauri |
| Tests | `cargo test --workspace` (≥10 test modules; terminal subsystem has 239 unit tests + Criterion benchmarks) | Vitest (unit), Playwright smoke + integration, `cargo test` for Rust; coverage via v8; Husky pre-commit/pre-push hooks |
| CI | (not deeply inspected) | GitHub Actions: `ci.yml`, `release.yml`, `release-stable.yml`, `auto-update-prs.yml` |
| Distribution | `target/release/nexus` + `nexus-tui` binaries | Tauri bundles (.app.tar.gz, deb/rpm/AppImage), public GitHub releases, dual-arch macOS artifacts (ADR 0083) |
| Quality gates | – | CodeScene code-health badges and ratcheted thresholds (ADR 0064) |
| Telemetry | – | Sentry + PostHog with release channels (alpha/stable/canary) and feature flags |

Tolaria is clearly further along the "shipping product" axis. Nexus is further along the "internal architecture" axis.

---

## 9. Maturity signals

**Nexus**

- Self-described v0.1.0 alpha. Workspace organization and ADR/PRD discipline are strong: 17 PRDs, an `IMPLEMENTATION_STATUS.md` that tiers each subsystem (✅/🟢/🟡), C4-model architecture doc with mermaid diagrams.
- Microkernel boundary is enforced and tests exist on the critical paths (terminal especially).
- Acknowledged gaps: WebSocket MCP transport, local embeddings, JS plugin tier, native chrome polish.
- The 80–100k LOC across ~20 crates is platform-grade for an alpha.

**Tolaria**

- Also v0.1.0 by `package.json`, but shipping public releases and badges (codecov, CodeScene, CI). README points at a Loom-walkthrough-driven onboarding flow and a public starter vault.
- ~90 ADRs. The cadence (numbered up into the high 80s, with several adopted in 2025/2026) suggests very active iteration.
- Co-located tests (`*.test.tsx` next to nearly every component), Playwright smoke + integration suites, Husky hooks, CodeScene gates.
- Deep attention to platform polish: window-state restoration, AppImage WebKit env workarounds, Linux custom titlebar, dual-arch macOS, multi-locale, telemetry, auto-update. These are the kinds of details that show up only when real users are running the app.

---

## 10. Where each one wins

**Pick Nexus's approach when you need:**

- A *platform* — a runtime you (or third parties) extend with plugins.
- Multiple surfaces (CLI, TUI, desktop, MCP) over the same data.
- Heavy queries: full-text over big vaults, graph traversal, RAG with embeddings.
- Strict capability/security boundaries because untrusted code (community plugins) might run.
- AI as a built-in feature, not a side process — including local models (Ollama / llama.cpp).

**Pick Tolaria's approach when you need:**

- A *finished* daily-driver app with a tight, opinionated UX.
- Git-as-history baked into the workflow, including a commit feed UI ("Pulse"), auto-commit, and conflict resolution.
- AI without holding API keys — let the user's existing CLI agents (Claude Code / Codex) do the work.
- Conventions over configuration: standard frontmatter field names trigger UI behavior automatically, which doubles as AI-readability.
- Lower engineering ceremony: one TS app + one Rust crate + one Node MCP server. No plugin sandbox to maintain.

## Convergence

Despite the gap, the two projects converge on a surprising number of design choices:

- Filesystem (markdown + YAML frontmatter) as the **single source of truth**, with all caches/indices being rebuildable from disk.
- Disk-first writes (state never updates before the IPC write resolves).
- An MCP server as the AI integration point.
- Tauri 2 + React for the desktop shell.
- ADR-driven design.

The core split is philosophical: Nexus says "make everything pluggable, including the kernel-to-surface boundary"; Tolaria says "make everything conventional, and let the AI ecosystem evolve outside the app."
