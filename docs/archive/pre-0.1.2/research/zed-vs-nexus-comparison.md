# Zed vs. Nexus — Capability Assessment

> **Purpose:** Compare Zed (the GPU-accelerated code editor by Zed Industries) with Nexus across all major capability areas, to surface competitive gaps, differentiated strengths, and potential feature directions.
>
> **Methodology:** Nexus analysis drawn from the live codebase (`crates/**`, `shell/src/**`, `docs/**`). Zed analysis drawn from verified research against Zed's public documentation, open-source repository, blog posts, and release notes current through Zed 1.0 (April 29, 2026).
>
> **Key framing:** These are fundamentally different products. Zed is a high-performance **code editor**; Nexus is a personal **knowledge management platform**. The overlap is real (terminal, git, AI, extensibility) but the design centers differ substantially.

---

## TL;DR

| | **Zed** | **Nexus** |
|---|---|---|
| One-line | GPU-accelerated collaborative code editor | Plugin-extensible knowledge runtime with CLI/TUI/desktop/MCP surfaces |
| Target user | Software engineers editing code | Knowledge workers managing markdown vaults |
| Core metaphor | Files in a project | Files in a "forge" (structured knowledge base) |
| Language | Rust + GPUI (custom GPU-accelerated framework) | Rust microkernel + TypeScript shell |
| Architecture | Monolithic editor process + WASM extensions | Microkernel + capability-gated IPC; 25-crate workspace; WASM plugin sandbox |
| Version / maturity | **v1.0 GA** (April 29, 2026); paying subscriber base | v0.1.0 alpha; personal-tool scope |
| Open source | Editor: GPL; GPUI framework: Apache 2.0; Zeta model: fully open-weight | Closed / personal-tool (alpha) |
| Pricing | Free (BYOK); Pro $10/mo ($5 token credits); Business tier | N/A (alpha) |
| Platforms | macOS, Linux, Windows (DirectX 11/Vulkan, Oct 2025) | macOS, Linux, Windows (Tauri 2 target) |
| AI strategy | Inline Assist, Parallel Agents, ACP for external agents, Zeta edit prediction | Streaming RAG chat, inline completion, agent planner, skills library, workflow automation |
| Collaboration | Real-time multiplayer via CRDTs, voice, channels (built-in) | CRDT conflict detection + resolution (Phase 4 shipped); no live multiplayer |
| Knowledge graph | None | 3-tier wikilink resolution, bidirectional backlinks, petgraph |
| Extension model | WASM extensions (~700) | Core (Rust) + Community (WASM) + Script (JS) plugins |
| Agent integration | Parallel Agents + ACP (host Claude Code, Gemini CLI, Codex, Cursor) | Agent planner with mid-plan approval + MCP host |
| Debugger | ✅ Integrated DAP (Rust, C/C++, JS, Go, Python) | ❌ |
| MCP | ✅ MCP server extensions; receives context | ✅ Full MCP server (13 tools) + host (McpClient, auto-discovery) |
| Surfaces | Desktop only (macOS/Linux/Windows) | CLI, TUI, Tauri desktop, MCP stdio server |

---

## 1. Product Vision & Target Use Case

### Zed

Zed is a **code editor** built for software development teams. Version 1.0 shipped April 29, 2026 after five years of development by the team that previously built the Atom editor (Nathan Sobo et al.). Its defining philosophy is speed — it renders at native GPU frame rates via its custom GPUI framework, has ~2 ms keystroke latency, and handles very large codebases without slowing down. Secondary pillars are **built-in real-time collaboration** (multiplayer editing, voice channels) and **AI-native workflows** (Parallel Agents, Agent Client Protocol, edit prediction).

Zed's scope is deliberately focused on code editing. It does not attempt to be a note-taking system, knowledge graph, or workflow automation engine.

### Nexus

Nexus is a **knowledge management platform** for managing markdown files (a "forge"). Its philosophy is **file-as-truth**: all content lives in plain markdown files on disk; every index (SQLite, Tantivy FTS, petgraph knowledge graph) is rebuildable from those files. A second pillar is **deep AI integration**: streaming RAG-augmented chat, inline editor completion, agent planner, skills library, and workflow automation are all first-class.

Nexus is closer to Obsidian or Logseq than to VS Code or Neovim. It has an editor and terminal, but those are features of the knowledge platform rather than the product's core identity.

---

## 2. Architecture Comparison

### 2.1 Core Design

| Dimension | Zed | Nexus |
|-----------|-----|-------|
| Process model | Single process with async tasks | Microkernel with IPC boundary between subsystems |
| UI framework | Custom GPUI (GPU-rendered, Rust; Apache 2.0) | Tauri 2 + React/Vite (WebView) for desktop; ratatui for TUI |
| Plugin isolation | WASM sandbox (extism) | WASM sandbox (wasmtime) + iframe sandbox for JS plugins |
| IPC | Internal async channels | Formal IPC: `context.ipc_call(plugin_id, command, args) → Result<Value>` |
| Capability gating | Extension-level trust | Per-call 14-type capability strings (`fs.read`, `net.http`, `process.spawn`, …) |
| Dependency enforcement | None enforced | `dep_invariants` test blocks kernel from depending on subsystems |

### 2.2 Extension/Plugin Systems

**Zed Extensions (~700 total):**
- WASM modules (sandboxed, cross-platform)
- Contribution types: language grammars (Tree-sitter), LSP adapters, DAP debugger adapters, theme extensions, icon theme extensions, snippet extensions, slash command extensions, MCP server extensions
- Cannot inject arbitrary UI (no panel/sidebar widget contributions beyond defined extension points)
- Registry at extensions.zed.dev, in-editor browsable
- No Lua/scripting: deliberately more constrained than Neovim for security and performance

**Nexus Plugins:**
- **Core (native Rust):** Full kernel access; capability-gated; 17 bundled core plugins registered in deterministic order
- **Community (WASM, wasmtime):** Capability-gated, hot-reloaded, crash-quarantine, reentrancy detection
- **Script (JS/TS):** Iframe-sandboxed in the shell at null-origin; PostMessage protocol
- Contribution points: commands, panels, menus, keybindings, sidebar tree providers, file handlers, URI handlers, webview panels, themes, theme snippets
- The shell starts **empty** — every visible UI element is a plugin contribution (equal footing for third-party vs. bundled)

**Assessment:** Zed's extension API is more stable and has a larger community ecosystem (700 vs. nascent). Nexus's plugin architecture is more powerful (arbitrary panel/UI contributions, three isolation tiers) but less mature and lacks a public marketplace. Zed's WASM extension sandbox is lighter-weight; Nexus's three-tier model (Core/Community/Script) offers finer-grained trust boundaries.

---

## 3. Editor Capabilities

### 3.1 Feature Matrix

| Feature | Zed | Nexus |
|---------|-----|-------|
| Primary format | Any text / code | Markdown + MDX (CommonMark + GFM + wikilinks + embeds + callouts) |
| Editor engine | Native GPUI text model | CodeMirror 6 (WebView) |
| Rendering performance | GPU-accelerated, 120 FPS; ~2 ms keystroke latency | WebView-bounded; comfortable for prose, not benchmarked at scale |
| Startup time | ~0.12–0.4 s; ~180–300 MB RAM | Heavier (Tauri + React hydration) |
| Multiple cursors | ✅ Full | ✅ CM6 native |
| Vim mode | ✅ First-class, high-fidelity (not a plugin) | 🟡 CM6 vim module (partial) |
| Language syntax highlighting | ✅ Tree-sitter (all major languages) | 🟡 CM6 language packs; markdown-first |
| LSP / go-to-definition | ✅ First-class, all major LSPs | ❌ Not implemented |
| Code actions / refactoring | ✅ Via LSP | ❌ Not implemented |
| Debugger (DAP) | ✅ Integrated: Rust, C/C++, JS, Go, Python; breakpoints, step, watches | ❌ |
| Multi-buffer editing | ✅ Unique: compose excerpts from N files in a single editable buffer | ❌ Single-file view |
| Outline / symbols view | ✅ Tree-sitter symbol outline | ✅ Heading-level Outline shell plugin |
| Inline diagnostics | ✅ Via LSP | ❌ |
| Block-level undo tree | ❌ Linear history | ✅ Undo forest (non-linear) |
| MDX component rendering | ❌ (raw text) | ✅ Self-closing + block-form MDX (`<Card />`, `<Card>…</Card>`) |
| Wikilinks | ❌ | ✅ 3-tier resolution + phantom-link upgrade |
| Inline AI completion | ✅ Inline Assist (selection → AI edit, multi-cursor) | ✅ `Mod+Shift+Space`, streams from `com.nexus.ai` |
| Edit prediction (tab-complete) | ✅ Zeta (open-weight), Copilot, Codestral, Mercury Coder, Sweep, Ollama | ❌ |
| Block-level IDs | ❌ | ✅ Stable `block_id` per heading/paragraph (ADR 0017) |
| Canvas / whiteboard | ❌ | ✅ Obsidian-compatible Canvas JSON |
| Jupyter / REPL | 🟡 Jupyter kernel integration (interactive REPL; .ipynb full editing in progress) | ❌ |

### 3.2 Editor Verdict

Zed is the clear winner for **code editing**: GPUI performance, LSP integration, multi-buffer, Tree-sitter grammars, integrated debugger, and Jupyter REPL are unmatched. Nexus leads on **markdown/knowledge editing**: wikilinks, block IDs, MDX components, undo forest, knowledge graph backlinks, and inline AI completion tailored to prose. These are non-overlapping strengths serving different workflows. The integrated debugger is a significant Nexus gap if it ever expands toward code editing.

---

## 4. AI Integration

### 4.1 Feature Matrix

| Feature | Zed | Nexus |
|---------|-----|-------|
| Edit prediction (tab-complete) | ✅ Zeta, Copilot, Codestral, Mercury Coder, Sweep, Ollama (6 providers) | ❌ |
| Inline AI edit | ✅ Inline Assist: selection → AI edit; multi-cursor support; @thread context injection | ✅ Streaming insert at cursor (`Mod+Shift+Space`) |
| Chat panel | ✅ Streaming, full message history editable in the panel | ✅ Session picker, multi-session storage |
| Parallel Agents | ✅ Multiple concurrent agent threads (Threads Sidebar); Git worktrees for isolation | ❌ (single agent session) |
| External agent hosting (ACP) | ✅ Claude Code, Gemini CLI, OpenAI Codex, Cursor as external ACP agents in Zed | ❌ |
| RAG / context injection | 🟡 @-mention context (files, dirs, symbols, diagnostics, rules files, threads) | ✅ Block-aware RAG: heading-context prefix, token budgeting, source citations |
| Semantic / vector search | ❌ | ✅ Embedding index (local or remote) |
| AI provider support | Anthropic, OpenAI, DeepSeek, Google Gemini, Ollama, any OpenAI-compatible API | Anthropic, OpenAI, Ollama, llama.cpp |
| Agent archetypes | ❌ | ✅ Writer / Coder / Researcher |
| Mid-plan approval | ❌ (auto-approve or step mode) | ✅ Preview → Approve → Step → Continue |
| Terminal AI assist | ✅ Terminal Assist (inline AI suggestions) | ✅ 5 built-in AI suggestion rules (cargo, npm, git, network, shell) |
| Skills / reusable prompts | 🟡 Slash commands and rules files | ✅ Skills library (YAML manifest, parameter substitution, built-in library) |
| Workflow automation | ❌ (tasks system for running commands) | ✅ Workflow engine (cron + file-event + manual triggers) |
| Privacy / secret redaction | ❌ | ✅ 6-pattern secret detector (AWS, GitHub PAT, PEM, etc.) in RAG pipeline |
| AI audit log | ❌ | ✅ All AI interactions logged; 90-day retention in SQLite |
| MCP server (expose forge) | 🟡 MCP server extensions (receives context; Zed as MCP client) | ✅ Full MCP server: 13 tools + note resources at `mcp://nexus/notes/<path>` |
| MCP host (spawn external) | 🟡 Consumes external MCP servers via extension | ✅ McpClient, auto-discovery, connection pool, agent auto-wiring |
| AI-generated commit messages | ✅ Calls configured LLM | ❌ |
| "New from Summary" | ✅ Fresh thread seeded by LLM summary of prior conversation | ❌ |

### 4.2 AI Verdict

Both products are deeply AI-integrated but along different axes:

**Zed's advantages:** Parallel Agents (multiple concurrent agentic threads with Git worktree isolation) is a major differentiator — no other editor has this natively. The Agent Client Protocol (ACP) enables Zed to host Claude Code, Gemini CLI, Codex, and Cursor as first-class external agents, giving users access to best-in-class external agents inside Zed's UI. Edit prediction with 6 provider options (including the open-source Zeta model for air-gapped use) is ahead of anything Nexus ships. Inline Assist with multi-cursor support enables coordinated AI edits across many positions simultaneously.

**Nexus's advantages:** RAG pipeline is deeper (block-level chunking with heading context, semantic search, token budgeting, source citations); Zed's @-mention context is file-level, not semantically ranked. Skills library (YAML manifests, parameterized prompts) is more structured than slash commands. Workflow automation (cron/file-event triggers, conditions, interpolation) has no Zed equivalent. Audit logging and secret redaction are enterprise-grade features Zed doesn't attempt. MCP server exposure (Nexus as a forge tool server for external clients) is a distinct capability from Zed's MCP client consumption.

The most actionable Nexus gap: no edit prediction (tab-complete), no parallel agent threads.

---

## 5. Agent Client Protocol (ACP) — Key Differentiator to Understand

Zed's ACP (announced January 2026, registry live April 2026) is conceptually important for Nexus to understand:

- ACP is an **open standard** (Apache 2.0) analogous to LSP: just as LSP decoupled language intelligence from editors, ACP decouples AI coding agents from editors
- External agents (Claude Code, Gemini CLI, OpenAI Codex, Cursor) register once in the ACP Registry and become available to every ACP-compatible editor client
- When a user runs Claude Code via ACP in Zed, the agent gets access to Zed's multi-file editing, multi-buffer reviews, full codebase context, and real-time edit visualization — while no code touches Zed's servers
- This is a fundamentally different model from Nexus's MCP host: Nexus's McpClient spawns external MCP servers that provide tools to Nexus's own agent; ACP inverts this — the external agent is the driver, Zed is the UI surface

**Implication for Nexus:** ACP positions Zed as a thin, fast editor surface that can front any best-in-class external agent. Nexus's agent system is self-contained (internal planner + skills + MCP tools). If external coding agents (Claude Code, Codex) become the dominant agent workflow, Nexus's internal agent system may need a way to delegate to them rather than compete. The MCP server Nexus already ships is the right primitive — external agents can call Nexus forge tools over MCP regardless of which editor they run in.

---

## 6. Collaboration

### 6.1 Feature Matrix

| Feature | Zed | Nexus |
|---------|-----|-------|
| Real-time co-editing | ✅ Built-in, presence cursors, shared view, sub-second latency | 🟡 CRDT conflict detection + resolution; no live multiplayer |
| Channels / voice | ✅ Built-in text channels + voice calls | ❌ |
| Guest access | ✅ Invite via link | ❌ |
| CRDT foundation | ✅ Yes (founding design) | ✅ Yes (Phase 4 shipped) |
| Follow mode (pair programming) | ✅ Follow collaborator's viewport | ❌ |
| Collaborative notes per channel | ✅ Markdown document per channel (CRDT-backed) | N/A |
| Git-based collaboration | 🟡 Basic git blame/diff | ✅ Deep git integration + auto-committer |
| Conflict resolution UI | ✅ Merge editor | 🟡 Conflict toast; per-block resolver modal deferred |

### 6.2 Collaboration Verdict

Zed has a significant lead in **real-time collaboration**: live co-editing with presence, follow mode, voice, and channel notes are production-grade features that Nexus hasn't started. Nexus has the CRDT substrate for it but no live session/presence layer. For **async collaboration** (git-based workflows), Nexus's 37-handler git IPC surface and auto-committer are more capable.

---

## 7. Terminal Integration

### 7.1 Feature Matrix

| Feature | Zed | Nexus |
|---------|-----|-------|
| Integrated terminal | ✅ Tab-based, GPU-rendered | ✅ PTY sessions, 50-session cap with LRU eviction |
| Session persistence | ❌ | ✅ SQLite-backed scrollback snapshots |
| Saved commands | ❌ | ✅ Named commands with slug/icon/shell overrides, sidebar |
| AI terminal assist | ✅ Inline AI suggestions for commands | ✅ 5 built-in AI suggestion rules (cargo, npm, git, network, shell) |
| Agent terminal use | ✅ Zed agents can execute terminal commands as part of agentic tasks | ✅ IPC action dispatcher routes workflow/agent steps |
| Memory monitoring | ❌ | ✅ RSS monitoring (soft/hard limits, 60-sample rolling history) |
| Process manager | ❌ | ✅ ManagedProcess FSM (Stopped/Starting/Running/Crashed/Restarting with backoff) |
| Ad-hoc command history | ❌ | ✅ SQLite-backed, `(command, working_dir)` dedup, run-count tracking |
| URL detection in output | ❌ | ✅ `detect_urls` |
| Pre-command runner | ❌ | ✅ Sentinel-based exit detection for chained pre-commands |
| Shell profile sourcing | ❌ | ✅ bash/sh/zsh/fish rc-file sourcing on session start |

### 7.2 Terminal Verdict

Nexus has a substantially more capable terminal subsystem: session persistence, saved commands, process lifecycle management, memory limits, and AI-powered suggestions for specific error patterns. Zed's terminal is simpler but sufficient for most development workflows; agents can use it as a tool. This is an area where Nexus has notably over-invested relative to its current maturity.

---

## 8. Git Integration

### 8.1 Feature Matrix

| Feature | Zed | Nexus |
|---------|-----|-------|
| Diff gutter | ✅ Inline, hover-to-see-hunk | ✅ (GitPanel) |
| Inline blame | ✅ | ✅ |
| Hunk-level staging | ✅ (stage individual hunks or files) | ✅ |
| Commit from editor | ✅ | ✅ |
| Push from editor | ✅ | ✅ |
| AI commit messages | ✅ LLM-generated | ❌ |
| Git Graph | ✅ Visual commit graph: lazy loading, resizable columns, search, remote branches | 🟡 Basic git log via IPC |
| Branch management | 🟡 (switch via status bar; create/delete via terminal) | ✅ Create/switch/delete/list |
| Stash | ❌ | ✅ |
| Tag management | ❌ | ✅ |
| SSH passphrase caching | ❌ | ✅ OS keyring |
| Auto-committer | ❌ | ✅ Background save auto-commits |
| Git events on kernel bus | N/A | ✅ state/branch/commit/dirty-change events |
| IPC surface | N/A (editor-native) | 37 handlers (`com.nexus.git`) |

### 8.2 Git Verdict

Both have strong git integration. Zed's Git Graph view and AI-generated commit messages are ahead of Nexus. Nexus's 37-handler IPC surface, stash, tag management, and auto-committer are ahead of Zed. For knowledge-base workflows where auto-commit + remote sync is the pattern, Nexus's approach is more fitting. For visual code review, Zed's inline diff UX and Git Graph are superior.

---

## 9. Search

### 9.1 Feature Matrix

| Feature | Zed | Nexus |
|---------|-----|-------|
| Fuzzy file finder | ✅ Fast, ranked | ✅ Command palette + file search |
| Project-wide text search | ✅ Ripgrep-backed; results appear in a **multibuffer** (editable in-place) | ✅ Tantivy FTS with scope operators |
| Multi-line search & replace | ✅ (recent addition) | ✅ |
| Regex search | ✅ | ✅ |
| Semantic / vector search | ❌ | ✅ Embedding index |
| Symbol search | ✅ (LSP + Tree-sitter) | ❌ |
| Search operators | ❌ (regex only) | ✅ `tag:`, `path:`, `prop:`, `type:` with post-filter |
| Knowledge graph traversal | ❌ | ✅ Backlinks, unresolved links, graph neighbors |
| Block-level indexing | ❌ | ✅ Each heading/paragraph indexed separately with heading-context prefix |

### 9.2 Search Verdict

Zed's multibuffer search result experience is a genuine differentiator — editing across search results in a unified editable buffer is more fluid than any conventional search panel. Its LSP/Tree-sitter symbol search is better for code navigation. Nexus's search is richer for knowledge navigation: operator-scoped FTS, semantic search, knowledge graph traversal, and block-level indexing provide capabilities that a code editor has no need for.

---

## 10. Structured Data & Knowledge Features

| Feature | Zed | Nexus |
|---------|-----|-------|
| Database views | ❌ | ✅ Table / Kanban / Calendar / Gallery (`.bases` format) |
| Property types | ❌ | ✅ Title, Select, Date, Number, MultiSelect, People |
| Filter operators | ❌ | ✅ eq/ne/gt/lt/gte/lte, contains, icontains, regex |
| Formula evaluator | ❌ | ✅ |
| CSV import/export | ❌ | ✅ |
| Canvas / whiteboard | ❌ | ✅ Obsidian-compatible Canvas JSON |
| Knowledge graph | ❌ | ✅ petgraph + bidirectional backlinks + phantom-link upgrade |
| Daily notes / journal | ❌ | 🟡 BL-001, not yet shipped |

This is a category where Nexus has no competition from Zed. Structured databases, Kanban boards, calendar views, and the knowledge graph are knowledge-management primitives that a code editor doesn't attempt.

---

## 11. Workflow Automation & Scripting

| Feature | Zed | Nexus |
|---------|-----|-------|
| Task runner | ✅ Tasks system: JSON-configured, shell commands, `$ZED_SYMBOL` variable, keybinding-invokable | 🟡 IPC action dispatcher |
| Workflow automation | ❌ | ✅ `.workflow.toml` with steps, conditions, variable interpolation |
| Cron triggers | ❌ | ✅ |
| File-event triggers | ❌ | 🟡 Scaffolded (in backlog) |
| Skills library | 🟡 Slash commands + rules files (less structured) | ✅ YAML-manifest skills with parameter substitution, built-in library |
| Agent archetypes | ❌ | ✅ Writer / Coder / Researcher |
| Mid-plan approval | ❌ (auto-approve or manual step) | ✅ Preview → Approve → Step → Continue |
| Execution history | ❌ | ✅ `.forge/agent/history/<plan_id>.json` |

Zed's task system is useful for running build commands, test suites, and code generation; it's not a workflow engine. Nexus's workflow engine (cron/file-event triggers, conditions, variable interpolation) and agent system (archetypes, mid-plan approval, execution history) have no Zed equivalent.

---

## 12. Security & Privacy

| Feature | Zed | Nexus |
|---------|-----|-------|
| Capability-gated extensions | 🟡 WASM sandbox; coarse extension-level trust | ✅ 14-type capability strings, per-call enforcement |
| Install-time consent | 🟡 Extension permissions (coarse) | ✅ HIGH-risk capabilities require consent dialog |
| Audit log | ❌ | ✅ 90-day retention; queryable via IPC |
| Credential vault | ❌ | ✅ OS keyring (`com.nexus.security`, 4 IPC handlers) |
| AI secret redaction | ❌ | ✅ 6-pattern detector before content enters RAG pipeline |
| Path traversal protection | ❌ | ✅ TOCTOU fixes + path-validation middleware |
| Air-gapped operation | 🟡 Ollama edit prediction; ACP external agents can avoid Zed servers | 🟡 Ollama/llama.cpp providers; no cloud dependency for core features |

---

## 13. Themes & UI Customization

| Feature | Zed | Nexus |
|---------|-----|-------|
| Theme format | JSON | CSS variables (547-token registry) |
| Bundled themes | Many (+ 100+ via community extensions) | 11 bundled themes |
| Plugin-contributed themes | ✅ Via extension | ✅ Theme snippets with cascade/override semantics |
| Live theme builder | ❌ | ✅ Build tab in ThemePicker: 26-variable editor, live colour pickers, TOML export |
| Hot-reload without restart | ✅ | ✅ |
| Density / spacing tokens | ❌ | ✅ `--nx-density-*` CSS variables |
| Icon themes | ✅ Separate configurable icon theme | ❌ |

---

## 14. Performance Characteristics

| Dimension | Zed | Nexus |
|-----------|-----|-------|
| Rendering | GPU-accelerated GPUI (Metal/Vulkan/DirectX); 120 FPS | WebView (Tauri); CPU-rendered |
| Keystroke latency | ~2 ms | No benchmark data; WebView-bounded |
| Startup time | ~0.12–0.4 s | Heavier (Tauri + React hydration) |
| Idle RAM | ~180–300 MB | Higher (WebView, SQLite, Tantivy) |
| Large file performance | ~8x faster vs. VS Code; native text model | CM6 is well-optimized; no data at scale |
| Index rebuild | N/A (no persistent index) | SQLite + Tantivy rebuild on cold start |
| Concurrent file watcher | N/A (project-scoped) | `notify` debouncer with atomic writes |

**Assessment:** Zed has a categorical raw performance advantage from GPUI. Nexus's Tauri/WebView architecture is comfortable for knowledge workloads (editing markdown, browsing a graph) but would feel sluggish on large codebases. This is an acceptable trade-off given Nexus's target use case but would be a problem if Nexus ever pivoted toward code editing.

---

## 15. Multi-Interface / Surface Coverage

| Surface | Zed | Nexus |
|---------|-----|-------|
| Desktop app | ✅ macOS, Linux, Windows (v1.0) | ✅ Tauri 2 desktop (macOS/Linux/Windows target; alpha) |
| CLI | 🟡 `zed` opens files; no headless/scriptable mode | ✅ `nexus` CLI: 14+ command groups, full IPC access |
| TUI (terminal UI) | ❌ | ✅ `nexus-tui` (ratatui): editor + terminal pane |
| MCP server (forge exposure) | ❌ (receives MCP; does not expose one) | ✅ 13 forge tools + note resources |
| Web | ❌ | ❌ (OPFS deferred) |
| Mobile | ❌ | ❌ (UniFFI deferred) |
| Remote SSH editing | ✅ Headless server; local UI, remote LSP/tasks/terminal | ❌ |
| External agent hosting (ACP) | ✅ Claude Code, Gemini CLI, Codex, Cursor run inside Zed | ❌ |

**Assessment:** Nexus has broader surface coverage (CLI, TUI, desktop, MCP) — a knowledge base needs to be accessible headlessly (scripts, automation, AI agents). Zed's remote SSH editing and ACP external agent hosting are significant advantages for development teams; Nexus has no equivalents.

---

## 16. Competitive Positioning Summary

### Where Zed Leads Nexus

1. **Editor performance** — GPUI is categorically faster than Tauri/WebView; 120 FPS, ~2 ms latency, ~8x faster large-file handling vs. VS Code
2. **Code editing features** — LSP, multi-buffer, Tree-sitter, inline diagnostics, symbol navigation
3. **Integrated debugger** — DAP-based, supports Rust/C++/JS/Go/Python; Nexus has nothing here
4. **Parallel Agents** — multiple concurrent agent threads with Git worktree isolation; no equivalent in any other editor, let alone Nexus
5. **Agent Client Protocol (ACP)** — open standard for hosting best-in-class external agents (Claude Code, Gemini CLI, Codex, Cursor) inside the editor; Nexus's agent is self-contained
6. **Edit prediction (tab-complete)** — 6 providers including Zeta (Zed's own open-weight model); fully local/air-gapped via Ollama
7. **Real-time collaboration** — production-grade multiplayer, presence, voice, follow mode, channel notes
8. **Extension ecosystem maturity** — ~700 extensions vs. nascent; stable extension API
9. **Remote SSH development** — headless server, local rendering, remote LSP/tasks/terminal
10. **Vim mode fidelity** — mature, widely tested, first-class (not a plugin)
11. **Git Graph** — visual commit graph with lazy loading, remote branches, search; AI commit messages
12. **Version and ecosystem maturity** — v1.0 GA; paying subscriber base; open-source (GPL + Apache 2.0)

### Where Nexus Leads Zed

1. **Knowledge graph** — bidirectional backlinks, 3-tier wikilink resolution, petgraph; Zed has no concept of a knowledge graph
2. **File-as-truth architecture** — markdown files always authoritative; indices rebuildable; no vendor lock-in
3. **RAG / knowledge AI** — block-level chunking with heading context, token budgeting, source citations, semantic search
4. **Agent with mid-plan approval** — Preview → Approve → Step → Continue with full execution history; Zed's agents are more autonomous
5. **Skills library** — YAML-manifest reusable prompts with parameter substitution; Zed has slash commands only
6. **Workflow automation** — `.workflow.toml` with cron/file-event triggers, conditions, variable interpolation; no Zed equivalent
7. **Structured databases** — `.bases` format with Table/Kanban/Calendar/Gallery views
8. **MCP server (forge exposure)** — Nexus exposes 13 forge tools over MCP so external agents (Claude Code, Cursor, etc.) can query the knowledge base; Zed receives MCP but doesn't expose its own
9. **Terminal depth** — session persistence, saved commands, process FSM, memory limits, AI suggestion rules
10. **Capability-gated security model** — per-call enforcement, 90-day audit log, credential vault, secret redaction before RAG
11. **Multi-surface parity** — CLI, TUI, desktop, and MCP all share the same IPC surface; Zed is desktop-only
12. **Plugin-first shell architecture** — shell starts empty; every UI element is a plugin contribution (equal footing for third-party vs. bundled)
13. **Theme builder** — live in-app 26-variable editor with colour pickers and TOML export

### Feature Gap Analysis — Priority Items for Nexus Roadmap

| Zed Feature | Priority Note |
|-------------|--------------|
| Parallel Agents | High signal: concurrent agent threads are demonstrably useful for large tasks; Nexus's single-session agent is the MVP |
| Edit prediction (tab-complete) | High for any user who codes; lower for pure note-takers; consider integrating Ollama FIM models |
| Real-time co-editing | CRDT substrate exists; the session/presence layer is the missing piece |
| Integrated debugger (DAP) | Low for knowledge workers; high if Nexus expands toward code editing (see BL-075-081) |
| ACP / external agent hosting | MCP server already enables this in reverse (external agents call Nexus); consider ACP client in Nexus's TUI/CLI |
| SSH remote project editing | Useful for technical users editing on servers |
| Git Graph | Visual commit graph is widely appreciated; feasible as a Nexus shell plugin |
| AI commit messages | Small but high-value; could wire to `com.nexus.ai` and `com.nexus.git` |
| Extension marketplace | Critical path for community growth; REQUIRED-FOR-FORMAL-RELEASE already calls this out |
| Multi-buffer editing | Could be adapted: compose note excerpts into a single view |

---

## 17. Strategic Observations

### For Nexus

1. **The editor overlap is narrow.** Nexus competes with Zed only on editor polish and vim mode — and loses on both. The bulk of Nexus's differentiation is orthogonal to Zed: knowledge graph, structured data, workflow automation, RAG. Don't chase Zed's editor strengths; deepen the knowledge-management moat instead.

2. **ACP vs. MCP: understand the positioning.** Zed's ACP allows external agents to use Zed as a UI shell. Nexus's MCP server allows external agents to use Nexus as a data source. These are complementary: an ACP-hosted Claude Code session running inside Zed could call Nexus's MCP server to read from the forge. Nexus should lean into being the **authoritative knowledge source** that external agents query, not a competitor to ACP-hosted agents.

3. **Real-time collaboration is the clearest gap.** The CRDT substrate is in place (Phase 4 shipped). The missing layer is a session/presence protocol for live co-editing. This is the one Zed capability that knowledge workers will notice and want.

4. **Parallel Agents are the next AI wave.** Zed's Parallel Agents (multiple concurrent threads, Git worktree isolation) represent a real step-change in agentic UX. Nexus's single-session agent model is the right starting point but will feel limited as users get comfortable with concurrent agents. Planning for a multi-session agent surface (even without worktree isolation) should be a near-term consideration.

5. **MCP server as a distribution channel.** Nexus's MCP server (13 tools) makes the forge accessible to Claude Code, Cursor, Zed, and any other MCP-capable client. This is a unique distribution surface: Nexus becomes the knowledge layer for the entire AI developer tool ecosystem, regardless of which editor someone uses.

6. **CLI and TUI parity is a moat.** Zed has no headless mode. Nexus's CLI can be scripted, automated, and called by agents. This serves the technical user segment that Nexus is currently targeting and enables workflows (cron automation, CI/CD integration, agent orchestration) that are impossible with a GUI-only tool.

7. **Plugin-first shell architecture inverts the plugin disadvantage.** Nexus lacks a large extension ecosystem, but its architecture means third-party plugins compete on equal architectural footing with bundled ones. This is a stronger foundation for community growth than bolting plugins onto a core product.

### Watching Zed's Trajectory

Zed is investing in AI and agentic workflows rapidly. The areas to watch:
- **ACP expansion** — as more agents implement ACP, Zed becomes the preferred surface for agentic coding; this extends rather than threatens Nexus's knowledge-layer position
- **Multi-buffer as knowledge tool** — multi-buffer could be adapted for note-composition workflows; worth watching if Zed moves toward prose editing
- **Extension API expansion** — if Zed allows panel/sidebar contributions, the architectural gap narrows
- **MCP server-side** — Zed currently only consumes MCP; if it ever exposes its project context as an MCP server, that would compete with Nexus's own MCP server for AI agent context

---

*Assessment date: 2026-05-09. Nexus snapshot: `IMPLEMENTATION_STATUS.md` 2026-05-06. Zed snapshot: Zed 1.0 (April 29, 2026), verified against zed.dev documentation, blog posts, and release notes.*
