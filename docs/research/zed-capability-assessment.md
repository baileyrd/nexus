# Zed Capability Assessment: What Nexus Has and What to Adopt

**Date:** 2026-05-14  
**Source:** [zed.dev](https://zed.dev) / [github.com/zed-industries/zed](https://github.com/zed-industries/zed)  
**Scope:** Feature-by-feature comparison of Zed's capabilities against Nexus; adoption recommendations ranked by priority.

---

## 1. Zed at a Glance

Zed is a high-performance, GPU-accelerated code editor written in Rust, built by the creators of Atom and Tree-sitter. Its defining traits are:

- **Performance**: 0.4 s startup, 180 MB idle RAM, 2 ms input latency, GPUI (custom GPU-rendered UI framework).
- **Multiplayer-first**: Real-time co-editing is a core primitive, not a plugin.
- **AI-native**: Inline editing, per-keystroke edit prediction, agent panel, external-agent hosting (ACP), and MCP client are all first-class.
- **Extension model**: ~800 WASM extensions (languages, themes, debugger adapters, context servers, MCP servers, slash commands).
- **Code-editor scope**: Zed is primarily a code editor. It does not have Nexus's knowledge-graph, canvas, database/bases, audio, or workflow subsystems.

---

## 2. Zed Full Feature Inventory

### 2.1 Editor Core

| Feature | Zed Notes |
|---------|-----------|
| Syntax highlighting | Tree-sitter (grammar-per-language, structural, not regex) |
| Multi-cursor editing | Full multi-cursor, cross-file via multibuffer |
| Code folding | LSP folding ranges with Tree-sitter fallback |
| Inlay hints | Parameter names, inferred types (LSP-driven) |
| Bracket matching | Rainbow brackets, auto-close, surround-selection |
| Auto-indent | Language-aware via Tree-sitter grammars |
| Snippets | Registered by extensions; `editor::InsertSnippet` action |
| Word wrap | Soft-wrap toggle per-buffer |
| Minimap | `ToggleMinimap` command |
| Code actions / quick fixes | Lightbulb icon + `editor: toggle code actions` |
| Rename symbol | `f2` — multi-file, results shown in multibuffer |
| Go to definition/references | `f12` / Cmd-Click; multiple definitions open in multibuffer |
| Hover docs | Type info + docs + resource links |
| Diagnostics | Per-file and project-wide panel (`cmd-shift-M`) |
| Breadcrumbs | File path + containing syntax nodes |
| Status bar | Error/warning counts, cursor position |
| Outline panel | Syntax-tree summary of current file (`cmd-shift-O`) |
| Format on save | Formatter configured per-language |

### 2.2 Multibuffers

A uniquely Zed editing paradigm: a single tab that holds **editable excerpts from multiple different files**. Multi-cursor edits can span the excerpts. Used for:
- Project-wide diagnostics view (every error in the project, editable in place)
- Find-all-references results (editable)
- Rename refactoring preview

### 2.3 Search

| Feature | Notes |
|---------|-------|
| Buffer search | Regex, case-insensitive (`(?i)` prefix or `alt-cmd-c`) |
| Project-wide search | Results in multibuffer; regex with `$0` capture-group syntax |
| Multi-line search/replace | Supported in both buffer and project search |
| Symbol search | Extension + command palette |
| File finder | Fuzzy file opening |

### 2.4 AI / Assistant

| Feature | Notes |
|---------|-------|
| **Inline Assistant** | Select code or terminal text, describe intent → in-place rewrite; multi-cursor aware |
| **Edit Prediction** | Per-keystroke completions; default provider is Zeta (Zed's open-source model); also GitHub Copilot, Codestral |
| **Agent Panel** | Persistent AI conversation with access to editor tools |
| **Multiple Threads** | `cmd-alt-j` sidebar; parallel agent threads, each with its own context window |
| **Multi-model** | Claude, GPT-4o, Gemini, local Ollama (Llama 3, Mistral, Phi, etc.) |
| **MCP client** | Connect to any MCP server; tools available to agent |
| **Agent tools** | Read files, write files, search codebase, run terminal commands, list diagnostics |
| **Slash commands** | `/file`, `/symbol`, `/diagnostics`, `/fetch`, `/now`, `/tab`, `/selection`, `/rules`, `/project` — inject context explicitly |
| **Context servers** | Extensions can register custom context providers |
| **External agents (ACP)** | Agent Client Protocol: run Claude Agent, Codex, Aider, OpenCode, Gemini CLI *inside* Zed — they can edit files and navigate code at editor speed |
| **Agent Server extensions** | Extensions can register AI agent servers (`AgentServerManifestEntry`) |

### 2.5 Real-Time Collaboration

| Feature | Notes |
|---------|-------|
| **Multiplayer co-editing** | Built-in, no extension needed; CRDT-backed |
| **Channels** | Persistent project rooms; hierarchical; public/private |
| **Voice chat** | Microphone auto-shared on channel join; mute toggle |
| **Screen sharing** | Share entire screen; collaborators open as a panel tab; auto-switches with follow mode |
| **Follow mode** | Per-pane following; follow cursor *and* screen share simultaneously |
| **Guest access** | Read-only access to shared projects |
| **Shared projects** | Any channel member can open a project shared into the channel |
| **Private calls** | Ad-hoc contacts-based calls separate from channels |

### 2.6 Remote Development

| Feature | Notes |
|---------|-------|
| SSH remote projects | Editor UI runs locally; codebase runs on remote server |
| Headless server mode | `zed --headless` on any SSH target |
| Remote terminal | Terminal in SSH session |
| Remote tasks | Run tasks.json tasks on the remote |
| Remote debugger | DAP over SSH |

### 2.7 Debugger (DAP)

| Feature | Notes |
|---------|-------|
| Debug Adapter Protocol | Standard DAP; adapters registered via extensions |
| Breakpoints | Set, conditional, log breakpoints |
| Step through code | Step in/over/out/continue |
| Variable inspection | Locals, call stack, watch expressions |
| Remote debugging | Via SSH or collaborative sessions (`DapStore` adapts) |
| Keybinding context | `debugger_stopped > vim_mode == normal` for Vim keybinding overrides |

### 2.8 Git Integration

| Feature | Notes |
|---------|-------|
| Project panel git status | File/dir names tinted by git state |
| Inline git blame | Current-line blame; multi-host (GitHub, GitLab, Gitea, Forgejo, Bitbucket, Codeberg, self-hosted) |
| Git panel | Community-contributed native panel with hunk staging |
| Diff views | Hunk-level diff inspection |
| Branch management | Switch, create, delete |
| Use as `$EDITOR` | `export EDITOR="zed --wait"` for commit messages |

### 2.9 Terminal

| Feature | Notes |
|---------|-------|
| Integrated terminal | Alacritty backend; `ctrl-\`` to open |
| Multiple terminal panes | Tabs, splits |
| Terminal AI assistance | Inline Assistant can rewrite terminal commands |

### 2.10 Tasks

| Feature | Notes |
|---------|-------|
| `tasks.json` system | Declare tasks per-project (run tests, build, lint, etc.) |
| Save-before-run config | `"save"` field controls pre-task save behavior |
| Shell integration | Tasks run in integrated terminal |

### 2.11 REPL

| Feature | Notes |
|---------|-------|
| Interactive REPL | Jupyter-style cell execution in editor buffers |
| Language kernels | Configurable per-language |
| Inline output | Results rendered inline in buffer |

### 2.12 Extension System

| Feature | Notes |
|---------|-------|
| Runtime | WebAssembly (WASM), written in Rust or any WASM-targeting language |
| Extension types | Languages, themes, debugger adapters, snippets, MCP servers, context servers, agent servers, slash commands |
| Stable ABI | `ExtensionIndex` with versioned contract |
| Count | ~800 extensions as of 2026 |
| Scaffolding | `zed extension new` |

### 2.13 Themes & Visual

| Feature | Notes |
|---------|-------|
| Themes | JSON-based; shipped built-in + via extensions |
| Font configuration | Fonts, sizes, ligatures |
| Minimap | Toggle; scrollbar integration |
| Configuration panel | GUI settings editor (in addition to JSON `settings.json`) |

### 2.14 Navigation & UI Panels

| Panel | Notes |
|-------|-------|
| Project panel | File tree; git status tinting; toggle with action |
| Outline panel | Syntax-tree document outline |
| Collaboration panel | Contacts, channels, calls |
| Diagnostics panel | Project-wide errors/warnings in multibuffer |
| Notification panel | Status bar button; dockable left/right |
| Threads sidebar | All AI agent threads grouped by project |

### 2.15 Vim Mode

- Full Vim emulation via `vim_mode: true` in `settings.json`
- Context-aware keymap overrides (e.g. different bindings when debugger stopped)
- Extensive modal emulation (normal, insert, visual, command)
- Active community cheatsheet

### 2.16 Performance & Platform

| Aspect | Value |
|--------|-------|
| Startup time | 0.4 s (vs VS Code 3.0 s) |
| Idle RAM | 180 MB (vs VS Code 650 MB) |
| Input latency | 2 ms (vs VS Code 12 ms) |
| Rendering | GPUI (custom GPU-accelerated UI framework in Rust) |
| Platforms | macOS, Linux (native); Windows in development |

---

## 3. Nexus Capability Inventory (Relevant to Comparison)

Based on the full codebase survey (330+ IPC handlers, 50+ UI plugins, 28 CLI groups):

| Domain | Nexus Status |
|--------|-------------|
| Editor (block-tree + CM6) | Full MDX block editor; inline AI completion; undo tree |
| LSP integration | 12 handlers: completions, hover, go-to-def, rename, diagnostics, format, code actions |
| Terminal | 25 handlers: PTY sessions, ANSI, scrollback, saved commands, AI suggestions, process monitoring |
| Git | 37 handlers: status, diff, stage/unstage hunks, commit, push/pull, branch, tag, stash, merge, rebase, cherry-pick, blame |
| AI chat / RAG | 22 handlers: streaming chat, multi-provider (Anthropic, OpenAI, Ollama), RAG pipeline, vector search, embeddings |
| Agent system | 25 handlers: 7 archetypes, planning, tool calling, step approval, memory |
| Skills | `.skill.md` with parameter binding, context matching, auto-trigger |
| Workflow | `.workflow.toml` with cron, webhooks, file events, git events, conditions, retry/backoff |
| MCP | Both client (connect external MCP servers) and server (expose 15 nexus_* tools) |
| Plugin system | Core Rust + WASM (wasmtime) community plugins with capability gating |
| Full-text search | Tantivy: tag:, path:, prop:, type: operators |
| Knowledge graph | Backlinks, outgoing links, unresolved links, graph visualization |
| Database/Bases | Table/Kanban/Calendar/Gallery; CSV, formulas, rollups, relations |
| Canvas | Obsidian-compatible whiteboard |
| Audio | STT (Whisper/OpenAI) + TTS (Piper/OpenAI) |
| Themes | 547-token CSS variable registry; 11 bundled themes; hot-reload |
| Comments | Block-anchored threads with resolution tracking |
| Security | OS keyring, capability enforcement, audit log |
| CRDT | RGA text merging, version vectors, git merge driver |
| Templates | Page template picker and insertion |
| Formats | Notion import/export, MDX/Canvas/Bases parsing |
| Link preview | URL metadata fetch |

---

## 4. Feature-by-Feature Comparison

### 4.1 Features Nexus Has (Comparable to Zed)

| Zed Feature | Nexus Equivalent | Parity |
|------------|-----------------|--------|
| LSP (completions, hover, go-to-def, rename, diagnostics) | `nexus-lsp` — 12 handlers | Near-parity |
| Integrated terminal (PTY, multi-session) | `nexus-terminal` — 25 handlers | Nexus richer (saved commands, AI suggestions, memory limits) |
| Git panel (status, blame, hunk staging, branches) | `nexus-git` — 37 handlers | Nexus more comprehensive |
| AI chat (multi-model, multi-session, RAG) | `nexus-ai` + `nexus-agent` | Nexus has more archetype depth and memory |
| MCP client | `nexus-mcp` client | Parity; Nexus also serves as MCP server |
| WASM extension/plugin system | `nexus-plugins` with wasmtime | Similar model; Nexus has capability gating |
| Full-text search | Tantivy with scoped operators | Nexus has more query operators |
| Themes | `nexus-theme` — 11 bundled; hot-reload | Comparable |
| Snippets | Registered via plugin contributions | Comparable |
| Plugin-contributed slash/context commands | Skills system + slash commands | Different names, similar intent |
| Find/replace (buffer + project) | `nexus-storage` find/replace | Comparable |
| Outline panel | Nexus editor CM6 outline panel | Comparable |
| Inline AI completion | `nexus-editor` "Complete at cursor" | Functional; less sophisticated than Zeta |
| Code actions / diagnostics panel | Via LSP + diagnostics UI | Comparable |
| Tasks / runnable processes | `nexus-workflow` + terminal | Nexus is more powerful (cron, conditions, webhooks) |

### 4.2 Features Zed Has That Nexus Lacks or Has Significant Gaps

| Zed Feature | Nexus Status | Gap Severity |
|------------|-------------|--------------|
| **Per-keystroke edit prediction** (Zeta/Copilot, every keystroke) | Nexus has on-demand inline completion only | **High** |
| **Debugger (DAP)** | Not present; no `nexus-debug` crate | **High** |
| **Real-time collaboration** (multiplayer co-editing UI) | CRDT exists but no live collab UI/server | **High** |
| **Remote development (SSH)** | Not present | **High** |
| **Multibuffers** (editable excerpts spanning files) | Not present | **Medium** |
| **External agent hosting (ACP)** | Not present; MCP tools only | **Medium** |
| **REPL** (Jupyter-style in-buffer) | Not present | **Medium** |
| **Voice chat / screen share in channels** | Not present | **Medium** |
| **Follow mode** (real-time cursor following) | Not present | **Medium** (depends on collab) |
| **Vim mode quality** | Basic in TUI; shell has no Vim mode | **Medium** |
| **Configuration GUI** (graphical settings editor) | Per-plugin config UI only | **Low–Medium** |
| **GPU-accelerated rendering** | Tauri/WebView (not GPUI) | **Low** (different use case) |
| **Language breadth** (100s of languages via extensions) | Markdown-first; ~5 code languages indexed | **Low** (scope difference) |
| **Tree-sitter in editor core** (syntax-aware editing) | Tree-sitter used for indexing; not deep editor integration | **Low–Medium** |

### 4.3 Features Nexus Has That Zed Lacks

These represent Nexus's differentiated value and should be protected, not diluted.

| Nexus Feature | Zed Status |
|--------------|-----------|
| Knowledge graph (backlinks, wikilinks, graph viz) | Not present |
| Database/Bases (Table/Kanban/Calendar/Gallery) | Not present |
| Canvas (Obsidian-compatible whiteboard) | Not present |
| Workflow system (declarative .workflow.toml, cron, webhooks) | Limited task runner only |
| Skills (.skill.md, context matching, auto-trigger) | Not present |
| Agent archetypes (Writer, Coder, Researcher, Auditor, Coach…) | Single generic agent |
| Agent memory (record/query/prune/export) | Not present |
| Audio (STT/TTS — Whisper, Piper, OpenAI) | Not present |
| Block-anchored comment threads | Not present |
| Notion import/export | Not present |
| MCP server (Nexus exposes tools to external clients) | Not present |
| Link preview (URL metadata cards) | Not present |
| OS keyring secrets management | Not present |
| Audit log (90-day retention, structured) | Not present |
| Vector search / semantic RAG over notes | Not present |
| CRDT git merge driver | Not present |
| Activity timeline (STT/TTS/chat interaction log) | Not present |
| Bases formula evaluator + rollups | Not present |

---

## 5. Adoption Recommendations

Ranked by expected user impact vs. implementation cost given Nexus's current architecture.

### Priority 1 — High Impact, Architecturally Feasible

#### 5.1 Per-Keystroke Edit Prediction

**What Zed does:** Every keystroke triggers an async prediction request; a single-/multi-line completion is shown as ghost text; `Tab` accepts. Default model is Zeta (open-source, trained on permissive data); optional Copilot/Codestral.

**Nexus gap:** `nexus-editor` has `inline_complete` (on-demand, user-triggered). There is no continuous prediction loop.

**Adoption path:**
1. Add a `prediction` IPC handler to `nexus-ai` that accepts `{prefix, suffix, language, file_path}` and returns completion text.
2. Wire a debounced (150 ms) CodeMirror 6 plugin in the editor that calls this handler and renders ghost text.
3. Support Ollama for local prediction (privacy-safe default).
4. Consider adopting the ADR pattern to document this in `docs/adr/`.

**Effort:** Medium. The AI plumbing and CM6 editor already exist; the missing piece is the prediction loop + ghost-text rendering.

#### 5.2 Debugger (DAP)

**What Zed does:** DAP over extensions; breakpoints, step through, variable inspection, remote debugging.

**Nexus gap:** No `nexus-debug` crate; users must debug in an external terminal.

**Adoption path:**
1. Create `crates/nexus-debug` as a `CorePlugin`.
2. Implement DAP client (reuse `dap` or `debug-adapter-protocol` crates from crates.io).
3. Register IPC handlers: `debug.start`, `debug.stop`, `debug.step_over/in/out`, `debug.set_breakpoint`, `debug.get_variables`.
4. Add a Debug panel plugin in `shell/src/plugins/nexus/debug/`.
5. Surface breakpoints in the CM6 editor gutter.

**Effort:** High, but high user value — especially paired with Nexus's existing terminal and LSP.

#### 5.3 Vim Mode in Desktop Shell

**What Zed does:** Full modal Vim emulation via `vim_mode: true`; context-aware keybinding overrides.

**Nexus gap:** The TUI has basic Vim-style navigation. The Tauri desktop shell editor has no Vim mode.

**Adoption path:**
1. Add a CodeMirror 6 `@codemirror/vim` extension to the editor plugin.
2. Expose a `vim_mode` setting in `nexus-editor`'s plugin settings schema.
3. Wire context signals (normal/insert/visual) to the kernel event bus so other shell plugins can adapt keybindings.

**Effort:** Low–Medium. `@codemirror/vim` is a well-maintained package; integration into the existing CM6 setup is straightforward.

---

### Priority 2 — Medium Impact, Significant Architecture Work

#### 5.4 Real-Time Collaboration UI

**What Zed does:** Built-in multiplayer; channels with voice, screen share, guest/write access; follow mode.

**Nexus gap:** `nexus-crdt` has the CRDT primitives (RGA, version vectors, op-log gossip) and the git merge driver, but there is no live collaboration server or UI.

**Adoption path:**
1. Add a WebSocket/WebRTC transport to the kernel's event bus (`watch: enable-transport` already exists in the CLI).
2. Create `crates/nexus-collab` as a `CorePlugin` implementing presence, cursor sharing, and awareness.
3. Add a Collaboration panel plugin in the desktop shell.
4. Phase 1: cursor presence + live document sync. Phase 2: voice (WebRTC). Phase 3: screen share (beyond typical note-app scope — may defer).

**Effort:** Very High. This is a platform-level undertaking. Prioritize document sync before voice/screen.

#### 5.5 Remote Forge via SSH

**What Zed does:** `zed --headless` on a remote machine; local UI connects via SSH.

**Nexus gap:** `NEXUS_FORGE_PATH` must be a local path.

**Adoption path:**
1. Introduce a `nexus-remote` crate that exposes the bootstrap runtime over a channel (WebSocket or stdio).
2. Allow `--forge-path ssh://user@host/path/to/forge` in the CLI.
3. The local Nexus shell connects to the remote bootstrap, proxying IPC calls.

**Effort:** High. Mirrors how Zed designed the headless server; Nexus's IPC-first architecture makes this tractable but non-trivial.

#### 5.6 Multibuffer / Multi-Excerpt Views

**What Zed does:** A single editable tab containing excerpts from multiple files; enables cross-file multi-cursor edits, project-wide diagnostics editing, and rename preview.

**Nexus gap:** Not present. Nexus has multi-note tabs but not excerpt-level aggregation.

**Adoption path:**
1. Extend `nexus-editor`'s block-tree model to support "excerpt" blocks that reference regions of other files by path + range.
2. Add IPC handlers: `editor.open_excerpts([{path, range}])` → returns a synthetic buffer.
3. Use this for the diagnostics view and find-all-references flow.

**Effort:** Medium–High. Fits well with Nexus's block-tree model conceptually.

#### 5.7 Agent Client Protocol (ACP) — Hosting External Agents

**What Zed does:** Via ACP, external AI agents (Claude Agent, Codex, Aider, Gemini CLI) can run inside Zed, editing files and navigating code at editor speed.

**Nexus gap:** Nexus's `nexus-mcp` exposes tools to external clients but does not host external agent loops.

**Adoption path:**
1. Define an ACP adapter in `nexus-agent` that translates ACP messages to IPC calls.
2. Allow external ACP agents to connect via the existing MCP/WebSocket transport.
3. Surface agent threads from external agents in the shell's Agent panel.

**Effort:** Medium. The MCP infrastructure provides a solid foundation.

#### 5.8 REPL / Interactive Evaluation

**What Zed does:** Jupyter-style cell execution in editor buffers with inline output.

**Nexus gap:** Nexus has a terminal and database formula evaluator but no REPL.

**Adoption path:**
1. Add a `repl` IPC handler to `nexus-terminal` that manages Jupyter kernels (via `jupyter_client` or subprocess).
2. Expose "run cell" as an MDX block type in `nexus-editor`.
3. Render output (text, images, tables) as block output nodes.

**Effort:** Medium. Nexus's block-tree MDX model is well-suited to inline output rendering.

---

### Priority 3 — Lower Priority / Specific Contexts

#### 5.9 Task Runner (tasks.json compatibility)

**What Zed does:** `tasks.json` — declare named shell tasks per-project; run from command palette.

**Nexus gap:** `nexus-workflow` is more powerful (cron, conditions, steps) but the simple "run this command" case is underserved compared to Zed's lightweight task system.

**Adoption:** Add a `tasks.toml` shorthand in `nexus-workflow` that maps a task name to a single shell command — compatible with simple use cases without requiring a full workflow definition.

**Effort:** Low.

#### 5.10 Graphical Configuration Editor

**What Zed does:** GUI settings panel (in addition to `settings.json`) with toggles, dropdowns, sliders.

**Nexus gap:** `nexus-theme` and per-plugin settings exist, but there is no central graphical settings panel.

**Adoption:** Add a Settings plugin to `shell/src/plugins/nexus/settings/` that iterates plugin schemas (already available via `nexus-bootstrap`) and renders a form UI.

**Effort:** Low–Medium.

#### 5.11 Multi-Line Search / Replace Improvements

**What Zed does:** Multi-line regex in both buffer search and project search.

**Nexus gap:** `nexus-storage`'s find/replace may not fully support multi-line regex across files.

**Adoption:** Verify `nexus-storage`'s `find_replace` handler supports multi-line patterns via Tantivy + a Rust regex pass; add if missing.

**Effort:** Low.

---

## 6. Features to Consciously Not Adopt

| Zed Feature | Reason to Skip |
|------------|----------------|
| GPUI (GPU-accelerated rendering) | Nexus uses Tauri/WebView; rewriting the UI framework is out of scope. Nexus's use case (markdown notes, databases, canvas) is not latency-critical in the same way. |
| 800-language extension ecosystem | Nexus is markdown-first. Deep multi-language code editing is not its primary use case. The existing ~5 indexed languages (Rust, TypeScript, Python, Go, JS) cover Nexus's own development needs. |
| Headless/streaming editor for terminal use | Nexus has a full-featured TUI (`nexus-tui`) and CLI that serve this role. |
| Built-in audio/video calling | Voice chat in channels is medium-priority at best; full audio/video would add significant complexity beyond Nexus's scope. |

---

## 7. Summary Table

| Zed Capability | Nexus Parity | Recommendation |
|---------------|-------------|----------------|
| Per-keystroke edit prediction | Partial (on-demand only) | **Adopt — P1** |
| DAP Debugger | None | **Adopt — P1** |
| Vim mode (desktop shell) | None | **Adopt — P1** |
| Real-time collaboration UI | Primitives only (CRDT) | **Adopt — P2** |
| SSH remote forge | None | **Adopt — P2** |
| Multibuffer / multi-excerpt views | None | **Adopt — P2** |
| ACP external agent hosting | Partial (MCP only) | **Adopt — P2** |
| REPL / in-buffer evaluation | None | **Adopt — P2** |
| Task runner (lightweight tasks.json) | Partial (workflow is heavier) | **Adopt — P3** |
| Configuration GUI | Partial (per-plugin only) | **Adopt — P3** |
| Multi-line search/replace | Likely partial | **Verify/fix — P3** |
| LSP features | Near-parity | Maintain |
| Terminal | Nexus richer | No action |
| Git integration | Nexus richer | No action |
| AI chat / RAG | Nexus richer | No action |
| Extension/plugin system | Comparable | No action |
| GPUI rendering | N/A — different stack | Skip |
| 800-language coverage | Out of scope | Skip |

---

## Sources

- [Zed Official Site](https://zed.dev)
- [Zed Features Page](https://zed.dev/features)
- [Zed AI Overview](https://zed.dev/docs/ai/overview)
- [Zed Agent Panel Docs](https://zed.dev/docs/ai/agent-panel)
- [Zed ACP Protocol](https://zed.dev/acp)
- [Zed External Agents](https://zed.dev/docs/ai/external-agents)
- [Zed Inline Assistant](https://zed.dev/docs/ai/inline-assistant)
- [Zed Debugger](https://zed.dev/docs/debugger)
- [Zed Remote Development](https://zed.dev/docs/remote-development)
- [Zed Collaboration Overview](https://zed.dev/docs/collaboration/overview)
- [Zed Channels](https://zed.dev/docs/collaboration/channels)
- [Zed Git Docs](https://zed.dev/docs/git)
- [Zed Project Panel](https://zed.dev/docs/project-panel)
- [Zed Multibuffers](https://zed.dev/docs/multibuffers)
- [Zed Outline Panel](https://zed.dev/docs/outline-panel)
- [Zed Vim Mode](https://zed.dev/docs/vim)
- [Zed Key Bindings](https://zed.dev/docs/key-bindings)
- [Zed Themes](https://zed.dev/docs/themes)
- [Zed Extension System Blog](https://zed.dev/blog/zed-decoded-extensions)
- [Zed Diagnostics](https://zed.dev/docs/diagnostics)
- [Zed All Actions Reference](https://zed.dev/docs/all-actions)
- [Zed All Settings Reference](https://zed.dev/docs/reference/all-settings)
- [Zed AI Tools](https://zed.dev/docs/ai/tools)
- [Zed IDE Complete Guide 2026](https://agmazon.com/blog/articles/technology/202603/zed-ide-complete-guide-en.html)
- [Is Zed Ready for AI Power Users 2026 — Builder.io](https://www.builder.io/blog/zed-ai-2026)
- [Zed vs VS Code 2026 — The Software Scout](https://thesoftwarescout.com/zed-vs-vs-code-2026-which-code-editor-should-you-choose/)
