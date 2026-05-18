# Per-Plugin Capabilities

> **As of:** 2026-05-17. Three sections: **(1)** the 23 in-tree backend core plugins (with the verbs each exposes); **(2)** the 17 shell core/chrome plugins; **(3)** the 56 first-party shell feature plugins under `shell/src/plugins/nexus/` — 51 implemented + 5 intentional stubs (see [`reference/todos.md`](reference/todos.md)). Sources: `crates/nexus-bootstrap/cap_matrix.toml`, each crate's `core_plugin.rs`, each shell plugin's `index.{ts,tsx}`.
>
> **Plugin total: 96** (23 backend + 17 shell core + 56 shell first-party).

The "Provides" column lists **what the plugin contributes** (handlers, UI surfaces, events, configuration). The "Requires" / capability column lists what it asks of others (capability gates).

---

## Section 1 — Backend core plugins (23)

Each row: plugin id, owning crate, handler count, capability surface, brief description. Full per-handler tables at [`ipc-handlers.md`](ipc-handlers.md); the security-cap inventory is at [`capabilities.md`](capabilities.md).

| Plugin id | Owning crate | Handlers | Caps required (callers) | Caps required (this plugin holds) | Events published | Provides |
|-----------|--------------|---------:|-------------------------|-----------------------------------|------------------|----------|
| `com.nexus.storage` | `nexus-storage` | 60 | downstream `fs.{read,write}` checks | `fs.{read,write}`, `fs.{read,write}.external` | `com.nexus.storage.file_{created,modified,deleted}` | File-as-truth fs ops; SQLite + Tantivy + tree-sitter symbol index; knowledge graph; bases SQL; canvas read/write; watcher; vector store; entity graph + Dream Cycle inbox |
| `com.nexus.git` | `nexus-git` | 38 | none (`push`/`push_tags` AUDIT-flagged for `net.http`) | `fs.{read,write}` (within forge), `net.http` for push | `com.nexus.git.{state, branch_changed, commit, dirty_changed}` | 38 verbs over the forge `.git/` via libgit2: status, log, branches, blame, diff, hunk staging, commit, push, branch+tag CRUD, merge / rebase / cherry-pick + abort, stash, conflict 3-way fetch, LFS |
| `com.nexus.terminal` | `nexus-terminal` | 32 | `process.spawn` on `create_session`/`repl_start`; AUDIT-flagged for `process.spawn` on `send_input` family | `process.spawn`, `fs.read`, `kv.{read,write}` | session lifecycle events (in-process mpsc) | PTY sessions, REPL kernels, ANSI render, saved/ad-hoc commands, cross-session search, pre-command runner, memory monitor, AI suggestions, job objects (Windows) |
| `com.nexus.ai` | `nexus-ai` | 28 | `ai.{chat, index, session.*, config.write, activity.write, tools.*}` | `net.http`, `net.http.localhost`, `kv.{read,write}` | AI activity events | Streaming chat/ask, RAG with citations, semantic search, file/entity enrichment, infer entity relations, tool loop, FIM predict, generate docs, sessions, set_config, status/config/activity, resolve_credentials |
| `com.nexus.dap` | `nexus-dap` | 21 | `process.spawn` on launch/attach; `protocol.host.contribute` on register | `process.spawn` | DAP events | Debug Adapter Protocol host; full session control (configuration_done, set_breakpoints, continue/next/step, threads, stack_trace, scopes, variables, evaluate) |
| `com.nexus.agent` | `nexus-agent` | 17 | `ai.chat` on `session_run`/`round_decide`; AUDIT-flagged for `ai.chat` on `delegate`/`plan` | `kv.{read,write}`, `ipc.call` (for any handler-as-tool), `fs.read` for transcripts | agent step events | Agent archetypes (Writer/Coder/Researcher + custom), plan + run, delegate, history, memory CRUD, transcript FTS5 search |
| `com.nexus.editor` | `nexus-editor` | 15 | none (mutations gated via storage fs.write) | downstream `fs.{read,write}` via storage | editor session events | CM6 editor sessions, block tree, transactions (insert/delete/merge/split), undo, multibuffer (open/refresh_excerpts), block-link resolve, embedded base views, sync after external change |
| `com.nexus.lsp` | `nexus-lsp` | 14 | `protocol.host.contribute` on register/unregister | `process.spawn` (LSP server boot, separate from handler caps) | LSP-server events | Language Server Protocol host; multi-server; verbs: completions, hover, definition, references, code_actions, format, rename, execute_command, document open/change/close |
| `com.nexus.workflow` | `nexus-workflow` | 12 | none (`run`/`run_digest` AUDIT-flagged — issue #77 laundering) | `ipc.call` (any handler in steps) | workflow run events | `.workflow.toml` registry, validate, cron + file_event triggers, digest pipeline, run history, action template seeding |
| `com.nexus.theme` | `nexus-theme` | 11 | none | downstream `fs.write` for forge-local settings | `theme_changed` | CSS variable cascade, 11 bundled themes, snippet enable/reorder, plugin overrides, mode toggle, live reload |
| `com.nexus.mcp.host` | `nexus-mcp` | 11 | `process.spawn` on connect; `protocol.host.contribute` on register | `process.spawn`, `net.http`, `net.http.localhost` | MCP server events | Connect external MCP servers (stdio + streamable-HTTP), list tools/prompts/resources, call_tool, dynamic-tool registry, plugin server contributions |
| `com.nexus.ai.runtime` | `nexus-ai-runtime` | 9 | `ai.runtime.{submit, control, observe}` | none directly | runtime task events | Unified AI/agent event loop, worker pool, task submit/cancel/pause/resume/get/list/events/pool_stats/wait_for (ADR 0028) |
| `com.nexus.skills` | `nexus-skills` | 8 | none | downstream `fs.read` for skill files | none | `.skill.md` registry, render/compose/invoke, context-aware listing, triggered_by phrase lookup |
| `com.nexus.security` | `nexus-security` | 7 | AUDIT-flagged for new `security.*` cap on `set_secret`/`delete_secret`/`clear_audit_log` | OS keyring access, fs.write for audit DB | audit log entries | OS keyring vault per-plugin namespace, audit log query/clear, kernel metrics snapshot, TLS pinning |
| `com.nexus.comments` | `nexus-comments` | 7 | none (downstream fs.write) | downstream `fs.write` | comment thread events | Block-anchored comment threads; JSON sidecar persistence; CRUD + resolve |
| `com.nexus.acp` | `nexus-acp` | 7 | `process.spawn` on initialize; `protocol.host.contribute` on register | `process.spawn` | ACP session events | Agent Client Protocol host; initialize / propose / accept / reject / disconnect |
| `com.nexus.database` | `nexus-database` | 6 | none (pure compute) | none | none | Property types (Title/Select/Date/Number/MultiSelect/People/timestamps), formula evaluator (64-level recursion), rollup, relation resolve, view pipeline (sort/filter/group), CSV import/export |
| `com.nexus.notifications` | `nexus-notifications` | 5 | `notifications.inbox.{read,write}`; downstream `ui.notify` for `send` | `net.http` (Discord/Telegram webhooks), SMTP via lettre, `fs.write` for inbox.db | inbox events | Desktop / Discord / Telegram / Email channels, SQLite inbox at `<forge>/.forge/notifications/inbox.db`, source-to-channel routing |
| `com.nexus.templates` | `nexus-templates` | 5 | none | downstream `fs.write` for apply | none | `.template.md` registry, parameter render, apply via storage, reload |
| `com.nexus.collab` | `nexus-collab` | 4 | none (`start_relay` AUDIT-flagged for `network.bind`) | `net.http` (relay client), bind 0.0.0.0 for relay server | `com.nexus.collab.presence_changed` | Live cursor/selection presence publication, WebSocket relay start/stop/status |
| `com.nexus.audio` | `nexus-audio` | 3 | `audio.record` on transcribe, `audio.synthesize` on synthesize | `net.http` (provider backend), microphone via OS | none | STT (local Whisper + OpenAI provider), TTS synthesize, backend status probe |
| `com.nexus.formats` | `nexus-formats` | 2 | none (pure parse/serialize) | downstream fs via storage | none | Notion archive import/export; underlying format library for markdown / canvas / `.bases` / forge config |
| `com.nexus.linkpreview` | `nexus-linkpreview` | 1 | none (AUDIT-flagged for `net.http`) | `net.http` | none | OG / Twitter card fetch (5 s timeout, 512 KB max body) |

**Totals:** 23 plugins · ~280 IPC handlers · 4 unique kernel-bus topic families (`com.nexus.git.*`, `com.nexus.storage.file_*`, `com.nexus.collab.*`, plugin-namespaced).

---

## Section 2 — Shell core / chrome plugins (17)

Source: `shell/src/plugins/core/<name>/index.ts`. These plugins make up the shell chrome itself; without them the shell would render nothing.

| Plugin id | Folder | Provides | What it does |
|-----------|--------|----------|---------------|
| `core.activity-bar` | `activityBar` | commands, view registry | Left-edge rail; view-switching icons; slot for plugin-contributed icons |
| `core.capabilityPrompt` | `capabilityPrompt` | views | Install-time + per-call permission consent overlay |
| `core.command-palette` | `commandPalette` | commands, keybindings (`Ctrl+Shift+P`, `Ctrl+P`), configuration | Searchable palette over all registered commands; configurable result limit |
| `core.configuration-service` | `configurationService` | (backend) | Settings registry + persistence; no UI |
| `core.editor-area` | `editorArea` | commands (close/next/prev/pin), keybindings (`Ctrl+W`, `Ctrl+Tab`, `Ctrl+Shift+Tab`) | Tab bar + central editor host region |
| `core.file-explorer` | `fileExplorer` | commands, keybindings, configuration | Sidebar file tree: new/open/refresh/hidden-files toggle |
| `core.filesystem-service` | `fileSystemService` | (backend) | High-level fs API surfacing `com.nexus.storage` to other shell plugins |
| `core.notification-service` | `notificationService` | configuration | Toast container, dismiss queue, auto-dismiss duration setting |
| `core.panel-area` | `panelArea` | commands, keybindings (`Ctrl+J`) | Bottom dock leaf host |
| `core.right-panel` | `rightPanel` | commands, keybindings (`Ctrl+Alt+B`) | Right dock leaf host |
| `core.settings` | `settings` | commands, keybindings (`Ctrl+,`) | Settings modal (Settings / Keybindings / Help tabs) |
| `core.sidebar` | `sidebar` | (stub) | Legacy sidebar shim — kept for backwards-compat |
| `core.terminal` | `terminal` | commands, keybindings (`Ctrl+\``), configuration | Integrated terminal pane chrome; font size + family settings |
| `core.theme-service` | `themeService` | (backend) | CSS variable resolver; theme switch wiring; no UI |
| `core.title-bar` | `titleBar` | commands | Window chrome (minimize / maximize / close) |
| `core.zoom` | `zoom` | commands, keybindings (`Ctrl+=`, `Ctrl+-`, `Ctrl+0`), configuration | UI zoom level; configurable step / min / max / default |

---

## Section 3 — Shell first-party feature plugins (51)

Source: `shell/src/plugins/nexus/<name>/index.ts`. `ls` shows 62 entries under `shell/src/plugins/nexus/`, but **10 of those are component dirs without an `index.ts`** (consumed by other plugins, not plugins themselves), and 1 is a shared module file (`constants.ts`). The remaining 51 are real plugins, all listed below. Note that **folder name ≠ plugin id** in a few cases (e.g. `activityTimeline/` declares `nexus.activity` per BL-052 rename).

| Plugin id | Folder | Provides | What it does |
|-----------|--------|----------|---------------|
| `nexus.activityBar` | `activityBar` | contextKeys | Activity bar context value (current selected view) |
| `nexus.activity` | `activityTimeline` | commands | Per-forge activity timeline pane (renamed from `nexus.activityTimeline` per BL-052) |
| `nexus.agent` | `agent` | commands | Show Agent view panel (driver UI for `com.nexus.agent`) |
| `nexus.ai` | `ai` | commands, keybindings (`Cmd+I`, `Ctrl+Alt+A`), configuration | Chat sidebar; provider/model config; clear + focus |
| `nexus.audio` | `audio` | commands, configuration | Web Speech transcribe/synthesize UI; language / voice / rate settings |
| `nexus.backlinks` | `backlinks` | commands | Focus Backlinks right-panel section |
| `nexus.bases` | `bases` | commands, keybindings | Database/table editor with undo/redo/cut/copy/paste |
| `nexus.canvas` | `canvas` | commands, keybindings, configuration | 2-D node-link canvas; export PNG/SVG/PDF, grid, help overlay |
| `nexus.collab` | `collab` | commands | Show Collaboration panel (driver UI for `com.nexus.collab`) |
| `nexus.commandPalette` | `commandPalette` | commands, keybindings (`Ctrl+Shift+P`/`Ctrl+P`), contextKeys, configuration | Result-limit-configurable command palette layered over `core.command-palette` |
| `nexus.comments` | `comments` | commands | Focus Comments right-panel section (driver UI for `com.nexus.comments`) |
| `nexus.confirm` | `confirm` | (utility) | Programmatic confirmation dialog |
| `nexus.crdtConflict` | `crdtConflict` | (utility) | CRDT merge conflict resolver toast / pick-side action |
| `nexus.diagnostics` | `diagnostics` | commands | Diagnostics pane; "open all in multibuffer" |
| `nexus.dreamCycle` | `dreamCycle` | commands | Dream Cycle inbox; refresh |
| `nexus.editor` | `editor` | commands, keybindings, contextKeys, settingsTabs | Tabbed editor; save / find / replace / blame; LSP rename / references; REPL kernel settings tab |
| `nexus.enrich` | `enrich` | commands, configuration | Force AI auto-enrichment of current file |
| `nexus.extensionsTab` | `extensionsTab` | settingsTabs | Extensions tab inside the settings modal |
| `nexus.files` | `files` | commands, keybindings, contextKeys | Files sidebar; new / rename / delete / reveal / copy path; `F2`/`Del` in tree |
| `nexus.gitPanel` | `gitPanel` | commands | Git Panel (status / diff / history tabs) |
| `nexus.gitStatus` | `gitStatus` | (utility) | Status-bar git indicator |
| `nexus.graph` | `graph` | (utility) | Knowledge graph right-panel section |
| `nexus.launcher` | `launcher` | (utility) | Workspace launcher screen |
| `nexus.linkSuggest` | `linkSuggest` | configuration | Inline `[[wiki-link]]` ghost suggestions while typing; debounce + score settings |
| `nexus.mcp` | `mcp` | commands | Show MCP Servers; refresh servers list |
| `nexus.memory` | `memory` | commands, configuration | Quick-capture overlay (default `Cmd+Shift+N`) with inbox/code capture |
| `nexus.multibufferSync` | `multibufferSync` | (utility) | Multibuffer diff/results sync |
| `nexus.notifications` | `notifications` | (utility) | Notification system backbone wiring |
| `nexus.notificationsInbox` | `notificationsInbox` | commands | Notification Center; refresh inbox |
| `nexus.notificationsSettings` | `notificationsSettings` | settingsTabs | Notifications tab in settings |
| `nexus.notion` | `notion` | commands | Import from Notion zip / export to Notion folder |
| `nexus.observability` | `observability` | (utility) | System observability backbone |
| `nexus.osArchitecture` | `osArchitecture` | (utility) | Architecture view panel |
| `nexus.outline` | `outline` | commands | Focus Outline right-panel section |
| `nexus.paneMode` | `paneMode` | commands, keybindings, contextKeys | Full-screen pane takeover; `Escape` exits |
| `nexus.pick` | `pick` | (utility) | Programmatic picker primitive |
| `nexus.pluginsMgmt` | `pluginsMgmt` | commands, keybindings (`Ctrl+Shift+X`), contextKeys | Manage Plugins modal: toggle / review / enable; `Escape` to close |
| `nexus.processes` | `processes` | commands, keybindings (`Ctrl+Shift+Y`) | Processes pane |
| `nexus.prompt` | `prompt` | (utility) | Programmatic prompt primitive |
| `nexus.recall` | `recall` | commands, keybindings, contextKeys, configuration | Recall-from-capture-notes overlay; default `Mod+Shift+R` |
| `nexus.rightPanel` | `rightPanel` | commands, keybindings (`Ctrl+Alt+R`), contextKeys | Right sidebar region; visibility context |
| `nexus.search` | `search` | commands, keybindings (`Ctrl+Shift+F`), configuration | Workspace search sidebar; result-limit setting |
| `nexus.semanticSearch` | `semanticSearch` | commands | "Search by Meaning" semantic similarity search |
| `nexus.sidebar` | `sidebar` | (stub) | Sidebar region wiring |
| `nexus.skills` | `skills` | commands | Skills panel; refresh skills list |
| `nexus.templates` | `templates` | commands | New-from-template / list / show panel / refresh |
| `nexus.terminal` | `terminal` | commands, keybindings (`Ctrl+\``), contextKeys, configuration | Integrated terminal; saved + history + cross-session search; emulator priority setting |
| `nexus.themePicker` | `themePicker` | commands, keybindings (`Ctrl+Shift+T`), contextKeys | Theme picker overlay (Themes / Snippets / Build tabs); `Escape` to close |
| `nexus.viewBuilder` | `viewBuilder` | (utility) | View-builder infrastructure (no direct UI) |
| `nexus.workflow` | `workflow` | commands | Workflows panel; refresh / validate TOML |
| `nexus.workspace` | `workspace` | commands, keybindings (`Ctrl+O`), contextKeys | Open folder / open remote forge / set root |

### Folders that are not plugins (5)

These folders exist under `shell/src/plugins/nexus/` but have **no `index.{ts,tsx}`** — they're component / type / helper directories consumed by sibling plugins, not registered with the host:

| Folder | Used by |
|--------|---------|
| `debugger/` | consumed by `nexus.editor`'s LSP/DAP integration |
| `healthPanel/` | helper component |
| `searchPanel/` | consumed by `nexus.search` |
| `status/` | helper module for `core.statusBar` slot |
| `statusBar/` | helper module for `core.statusBar` slot |

Plus the shared module file `constants.ts` (cap/threshold table referenced by several plugins).

### Stub plugins (5 — renders "Not yet implemented")

These ARE registered plugins, but their pane body is currently a placeholder. They claim a slot in the chrome but await implementation. See [`reference/todos.md`](reference/todos.md) §6 for the planned behaviour of each:

| Plugin id | Folder | Intended feature |
|-----------|--------|------------------|
| `nexus.allProperties` | `allProperties/index.tsx` | List every frontmatter property on the active note |
| `nexus.bookmarks` | `bookmarks/index.tsx` | List saved bookmarks |
| `nexus.fileProperties` | `fileProperties/index.tsx` | Show the active note's file properties |
| `nexus.outgoingLinks` | `outgoingLinks/index.tsx` | List outgoing links from the current buffer |
| `nexus.tags` | `tags/index.tsx` | Surface the active note's tags |

### Count reconciliation

`ls` count: 51 implemented + 5 stub plugins + 5 component-only folders + 1 `constants.ts` file = **62 entries**.

Plugin total across the system: 23 backend + 17 shell core + 56 shell first-party (51 + 5 stubs) = **96**.

---

## Cross-section: where features come from

| Feature | Backend handler(s) | Shell driver plugin |
|---------|---------------------|---------------------|
| Open / save a note | `com.nexus.storage::{read_file, write_file}` + `com.nexus.editor::{open, save}` | `nexus.editor` |
| Search the forge | `com.nexus.storage::search` | `nexus.search` (sidebar), `nexus.searchPanel` (panel) |
| Git status / commit / push | `com.nexus.git::*` | `nexus.gitPanel`, `nexus.gitStatus` |
| Terminal / saved cmds | `com.nexus.terminal::*` | `nexus.terminal`, `core.terminal` |
| Chat with AI | `com.nexus.ai::stream_chat` | `nexus.ai` |
| Run an agent | `com.nexus.agent::session_run` + `com.nexus.ai.runtime::*` | `nexus.agent` |
| Bases view + edit | `com.nexus.database::apply_view` + `com.nexus.storage::base_*` | `nexus.bases` |
| Canvas | `com.nexus.storage::{canvas_read, canvas_write, canvas_patch}` | `nexus.canvas` |
| Notifications inbox | `com.nexus.notifications::inbox_*` | `nexus.notificationsInbox` |
| Theme switching | `com.nexus.theme::*` | `nexus.themePicker` |
| Skills | `com.nexus.skills::*` | `nexus.skills` |
| Workflows | `com.nexus.workflow::*` | `nexus.workflow` |
| Live collab | `com.nexus.collab::*` | `nexus.collab` |
| Comments | `com.nexus.comments::*` | `nexus.comments` |
| Quick capture | `com.nexus.storage::create_file` + KV | `nexus.memory` |
| Outline / backlinks / tags | `com.nexus.storage::{query_blocks, backlinks, query_tags}` | `nexus.outline`, `nexus.backlinks`, `nexus.tags` |
| MCP server management | `com.nexus.mcp.host::*` | `nexus.mcp` |
| Plugin install / grants | shell-side `scan_plugin_directory` + `set_plugin_granted_capabilities` | `nexus.pluginsMgmt` |
| Audio STT/TTS | `com.nexus.audio::*` | `nexus.audio` |

---

## How a new capability lands in both backend and shell

1. Add the IPC handler in the right `nexus-<service>` crate (`core_plugin.rs::dispatch`).
2. Add a `[[handler]]` row in `crates/nexus-bootstrap/cap_matrix.toml`.
3. Regenerate ts bindings + schemas via `scripts/check_ipc_drift.sh`.
4. Add a shell plugin under `shell/src/plugins/nexus/<feature>/index.ts` that calls the handler via `ctx.ipc.call(plugin_id, command, args)` and contributes a UI surface (`commands` / `panels` / `keybindings` / etc.).
5. List both the backend handler (in Section 1) and the shell driver (in Section 3) in this file. New capabilities **must** appear in both sections; an unsurfaced backend handler usually means it's dead weight.
