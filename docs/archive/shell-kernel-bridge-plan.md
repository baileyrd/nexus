> **Archived 2026-04-26** — Living implementation plan for the shell ↔ kernel bridge (Phase 0–4). Phases shipped; the legacy `crates/nexus-app` was deleted under Phase 4 WI-37.

# Shell ↔ Kernel Bridge — Implementation Plan

Living document. Each phase commit updates the relevant section in-place; finished phases move to the _Delivered_ section at the bottom.

## Status

Branch: `feat/ui-shell-rebuild` (merged; legacy `crates/nexus-app` deleted under Phase 4 WI-37 on 2026-04-24). Pushed through `1ef3d81`; local commits ahead through `f7ad739` plus pending. Phase 0 (bridge infra) and Phase 1 (retire standalone commands) shipped. Phase 2 is 15 plugins deep — editor (read-only → markdown → tabs → edit/save), command palette, search, outline (+ scroll-spy), backlinks, graph, terminal, AI chat, pane-mode infra, plugins-mgmt, processes, workflows, skills, MCP, agent. Phase 3 polish underway: design-bundle layout widths + statusbar height aligned, typed icon module ported. Tree is clean; the React plugin layer now talks to the kernel via `api.kernel.invoke` / `api.kernel.on` for every backend call except the pre-kernel `path_exists` (workspace verification during launcher restore).

See the **Delivered** section at the bottom for the full commit list. Queued items for the next session:

- **Pane-mode plugins** (infra landed in `4504667`): ~~`nexus.agent`~~ shipped paneMode v1; `nexus.templates` blocked on a real kernel template source.
- **Sidebar plugins**: `nexus.tasks` (`com.nexus.storage::task_query` — kernel handler not yet present, blocked).
- **Overlays**: settings panel (`core.settings`) is now wired into `main.tsx` and reachable via Ctrl+, — `core.configuration-service` was already there; the missing piece was registering the UI plugin and contributing real schemas. `nexus.editor` ships the first two settings (`confirmCloseDirty`, `defaultMode`); other plugins migrate as they need user-tunable behaviour.
- **Specialty**: ~~`nexus.mcp`~~ shipped sidebar + tool-call modal.
- **Editor polish**: outline ↔ editor wiring is bidirectional now — `editor:scrollToHeading` (`7715730`) plus `editor:activeHeadingChanged` scroll-spy that highlights and auto-scrolls the active outline row. Next polish item TBD.
- **Visual polish** (Phase 3): icon module ported, layout widths aligned, titlebar cluster shipped — see Phase 3 section. Activity-bar items now all use `iconName`; no `iconPath` callers left in `shell/src/plugins/`.
- **UX polish**: replaced `window.confirm` with a styled in-app dialog routed through `api.input.confirm`. `api.input.prompt` is still the platform fallback — same migration when needed.

Upstream dependencies (not blocking shell work):

- `build_shell_runtime` in `nexus-bootstrap` so the kernel stops booting under `com.nexus.cli` invoker identity.
- `list_plugins` IPC in `nexus-plugin-api` so the processes pane can see kernel-side plugin state.
- `com.nexus.terminal.*` topic events so terminal output stops being poll-driven.
- PTY resize handler in `nexus-terminal` so xterm resize propagates to the shell process.
- `com.nexus.ai.*` streaming events so `nexus.ai` can switch from one-shot `ask` to token-by-token render.

## Decisions

| # | Question | Answer |
|---|---|---|
| 1 | First launch UX | Obsidian-style launcher modal (recents list + Create / Open / Clone actions). Shell boots with no kernel; kernel boots on workspace pick. |
| 2 | Storage backend | **Option D — match the legacy `crates/nexus-app`**: single JSON file at `<app_config_dir>/shell-state.json`, read once at startup, atomic write (tmp → rename). Per-forge UI state keyed by absolute path. Kernel KV stays for kernel-plugin state only. localStorage + zustand-persist phased out as plugins migrate. |
| 3 | Fate of `crates/nexus-app` | **3c — keep during transition, delete at parity.** Shipped: deleted under Phase 4 WI-37 on 2026-04-24. |
| 4 | Plan as doc | **Yes** — this file. Updated in the same commit whenever a phase lands. |

## Architecture

```
Tauri window (webview)
 ├─ React shell + EventBus (JS)          ← existing plugin system, unchanged contract
 ├─ api.kernel.invoke(pluginId, cmd, args) → Promise<Value>
 ├─ api.kernel.on(topic, handler)         ← wraps Tauri event listener
 └─ api.storage (reads/writes the shell-state.json via bridge commands)
               │                                │
         tauri::invoke                   tauri::event emit
               │                                │
Tauri Rust (shell/src-tauri)
 ├─ persistence.rs   — shell-state.json load/save + forge-path helpers
 ├─ bridge.rs        — KernelRuntime managed state (uninit until workspace pick)
 ├─ #[command] boot_kernel(forge_root)            — runs nexus_bootstrap::build
 ├─ #[command] kernel_invoke(pluginId, cmd, args) — context.ipc_call(..)
 ├─ #[command] kernel_subscribe(topicPrefix)      — spawns forwarder task
 └─ #[command] kernel_unsubscribe(id)
               │
Nexus kernel (nexus-bootstrap::build)
 ├─ EventBus (tokio broadcast, JSON payloads, reverse-DNS topics)
 ├─ 12 core plugins (storage, git, editor, ai, workflow, agent, skills, mcp,
 │                   terminal, theme, database, security)
 └─ KvStore (.forge/kv.sqlite3, plugin-scoped)
```

**Invariant:** shell plugins are JS clients. They never link Rust, never implement `CorePlugin`. They invoke kernel handlers and subscribe to kernel topics through three new bridge primitives on `PluginAPI`.

---

## Phase 0 — Bridge infrastructure · **Delivered**

Foundations before any kernel-backed feature can land.

Landed across `21509d0` (launcher + persistence) → `2581e8e` (path deps + empty `KernelRuntime`) → `36d7672` (`init_forge` / `boot_kernel` / `shutdown_kernel` + window-close hook) → `3ee56c5` (`kernel_invoke` / `kernel_subscribe` / `kernel_unsubscribe` + `api.kernel` frontend) → `9323cd5` (workspace wires kernel lifecycle into every state transition). Adjustments: `5e354b1` hoisted the design tokens into `index.html` because Vite's JS-injected `shell.css` was dropping the `:root` block on documentElement.

**Steps:**

1. **Persistence** — ported from the legacy shell's `persistence.rs` (now deleted) into [`shell/src-tauri/src/persistence.rs`](../shell/src-tauri/src/persistence.rs). Expose Tauri commands `get_shell_state()`, `save_shell_state(state)`, `write_last_forge_path(path)`, `read_last_forge_path()`. Shell-side wrapper: load once at startup into a Zustand store; writes debounced ~500ms to `save_shell_state`.
2. **`init_forge` Tauri command** — thin wrapper around `nexus_bootstrap::init_forge(path)`. Called on Create / Open when `.forge/` is absent.
3. **`nexus.launcher` plugin** — pre-kernel UI, rendered when `nexus.workspace.rootPath` is empty. Left column: recents list (from persistence). Right column: Create / Open actions (Clone stubbed). On pick: `init_forge` → persist → set workspace → shell renders over the launcher.
4. **Bridge scaffold** — `shell/src-tauri/src/bridge.rs` with `KernelRuntime` managed state (`Arc<Mutex<Option<Runtime>>>`). Starts empty. All kernel-facing commands return `"no workspace open"` until `boot_kernel` fires.
5. **Path deps** — add to [shell/src-tauri/Cargo.toml](../shell/src-tauri/Cargo.toml): `nexus-kernel`, `nexus-bootstrap`, `nexus-plugin-api`, `nexus-plugins` (all via `path = "../../crates/…"`). `exclude = ["shell"]` in root stays.
6. **`boot_kernel(forge_root)`** — runs `nexus_bootstrap::build` on Tauri's tokio runtime, stashes `Runtime` into managed state. Called from the launcher's pick flow.
7. **`kernel_invoke / kernel_subscribe / kernel_unsubscribe`** — JSON envelopes over `context.ipc_call` and `bus.subscribe(EventFilter::CustomPrefix)`. Subscriptions track JoinHandles in a `Map<SubscriptionId, JoinHandle>`; frontend cleans up on plugin deactivate.
8. **`api.kernel` surface** — extend [types/plugin.ts](../shell/src/types/plugin.ts):
   ```ts
   kernel: {
     invoke<T>(pluginId: string, commandId: string, args?: unknown, timeoutMs?: number): Promise<T>
     on<T>(topicPrefix: string, handler: (payload: T) => void): Promise<() => void>
     available(): boolean  // false until boot_kernel succeeds
   }
   ```
9. **Shutdown hook** — Tauri `WindowEvent::CloseRequested` → `runtime.kernel.shutdown().await; loader.shutdown().await;` before drop.

**Acceptance:**

- Launcher appears on first launch. Recents persist across restarts via `shell-state.json`.
- Picking a folder with no `.forge/` auto-initialises it (`notes/`, `attachments/`, `.forge/kv.sqlite3`).
- After workspace pick, kernel is booted; `api.kernel.available()` returns `true`.
- Test plugin can `await api.kernel.invoke('com.nexus.storage', 'list_dir', { relpath: '' })` and get the same shape as current `read_dir`.
- Closing the window calls `kernel.shutdown()`; no background tokio tasks leak.

---

## Phase 1 — Retire standalone commands, route through kernel · **Delivered**

Mechanical replacement. Each plugin flips one `invoke('…')` to `api.kernel.invoke('com.nexus.*', '…')`.

`0fee389` + `ccc99c6` moved `nexus.files` to `com.nexus.storage::list_dir` and wired the `file_*` event subscriptions. `186a6b0` moved `nexus.gitStatus` to `com.nexus.git::status` with the four `com.nexus.git.*` topics covered by a single prefix listener; `git2` dropped from `shell/src-tauri/Cargo.toml`. `path_exists` stayed as planned.

- `read_dir` → `com.nexus.storage::list_dir`. Delete `read_dir` from [shell/src-tauri/src/lib.rs](../shell/src-tauri/src/lib.rs).
- `get_git_status` → `com.nexus.git::status`. Delete the `git2` dep and standalone command.
- `path_exists` stays — trivial, used pre-kernel for workspace verification.
- `nexus.files` subscribes to `com.nexus.storage.file_created / file_modified / file_deleted` for live refresh instead of manual refetch.
- `nexus.gitStatus` subscribes to `com.nexus.git.branch_changed / commit / dirty_changed` for live updates.

**Acceptance:** same observable behaviour with standalone commands removed from lib.rs. File-tree + git status update live as files change / branches move.

---

## Phase 2 — Feature plugins (per design bundle layout)

Each is a self-contained plugin. Order by dependency + value. `Status` column tracks commit or "queued".

| # | Plugin | Kernel-side | Slots | Design ref | Status |
|---|---|---|---|---|---|
| 1 | `nexus.editor` | `com.nexus.storage::read_file/write_file` (no `com.nexus.editor` handlers used yet) | `editorArea` | [forge_doc.jsx](../.design-bundle/project/forge_doc.jsx) | `0a736a7` read-only · `6a7ac5b` markdown · `ca1825c` tabs · `f497656` edit+save+dirty · `7715730` scroll-to-heading |
| 2 | `nexus.outline` | editor content + local parser | `rightPanelContent` (Outline tab) | [forge_panels.jsx](../.design-bundle/project/forge_panels.jsx) | `32c7556` (+ `7715730` scroll wire) |
| 3 | `nexus.search` | `com.nexus.storage::search` (Tantivy) | `activityBar`, `sidebarContent` | forge_panels | `cced076` |
| 4 | `nexus.backlinks` | `com.nexus.storage::backlinks` | `rightPanelContent` (Backlinks tab) | forge_panels | `e5929b4` |
| 5 | `nexus.graph` | `com.nexus.storage::outgoing_links` + `backlinks` | `rightPanelContent` (Graph tab) | forge_panels | `7414f15` |
| 6 | `nexus.commandPalette` | aggregated commands | `overlay` (⌘⇧P) | Forge PALETTE data | `efc0c90` (+ `09e31da` keybinding-pill backfill) |
| 7 | `nexus.terminal` | `com.nexus.terminal::create_session/send_input/pump` | `panelArea` | [forge_processes.jsx](../.design-bundle/project/forge_processes.jsx) | `08d6a66` |
| 8 | `nexus.processes` | `com.nexus.terminal::list_sessions` + `com.nexus.mcp.host::list_servers` + shell-side `pluginList` + 9 event prefixes | `paneMode` (full-pane) | forge_processes | `1ef3d81` |
| 9 | `nexus.tasks` | `com.nexus.storage::task_query` | `activityBar`, `sidebarContent` | — | queued |
| 10 | `nexus.ai` | `com.nexus.ai::ask` (stateless RAG; streaming deferred) | `sidebarContent` | [forge_orchestrate.jsx](../.design-bundle/project/forge_orchestrate.jsx) (partial) | `4694bef` (sidebar v1 — streaming + full-pane are follow-ups) |
| 11 | `nexus.agent` | `com.nexus.agent::plan/run/run_plan/execute_step/history_list/history_get/history_delete` + `com.nexus.agent.*` topic stream | `paneMode` (full-pane) | forge_orchestrate | `29563bc` paneMode v1 + `49b961e` history-delete + `3223337` step-by-step approval |
| 12 | `nexus.workflow` | `com.nexus.workflow::list/run` | `activityBar`, `sidebarContent` | — | `6c86e92` sidebar v1 — list + manual run; full-pane is a follow-up |
| 13 | `nexus.templates` | kernel read | `paneMode` (full-pane) | [forge_templates.jsx](../.design-bundle/project/forge_templates.jsx) | queued |
| 14 | `nexus.pluginsMgmt` | `pluginList` + `communityPluginManifests` services | `overlay` (⌘⇧X) | — | `b168e9a` |
| 15 | `nexus.paneMode` (infra) | — | new `paneMode` SlotId | — | `4504667` |
| 16 | `nexus.skills` | `com.nexus.skills::list` (full Skill struct including body) | `activityBar`, `sidebarContent` | — | `7d0ac8b` sidebar v1 — list + inline expand panel |
| 17 | `nexus.mcp` | `com.nexus.mcp.host::list_servers/connect/disconnect/list_tools/list_resources/list_prompts/call_tool` | `activityBar`, `sidebarContent`, `overlay` | — | `f7ad739` sidebar v1 + `d677d01` tool-call modal |
| 18 | `nexus.settings` | TBD | overlay | — | queued |

Pane-mode plugins (terminal/processes, ai/agent, templates) mirror the design's `pane === 'ai' | 'terminal' | 'templates'` flag — when their activity icon is active the tri-pane is replaced with a full-window workspace view.

**Acceptance per plugin:** observable feature parity with the legacy `crates/nexus-app`'s equivalent panel (now deleted — see service-crate `core_plugin.rs` IPC handlers), end-to-end data real, no hardcoded content, tokens via CSS vars.

---

## Phase 3 — Visual polish

Already done: `shell.css :root` carries the exact oklch palette from the design bundle. Memory `feedback_design_bundle_source_of_truth.md` enforces token-only usage.

Remaining:
- **Icon module** `shell/src/icons/` — landed in `7bce519`: all 27 glyphs from [forge_icons.jsx](../.design-bundle/project/forge_icons.jsx) plus `refresh` + `play`. Typed `IconName` union and a single `<Icon name="…" size={16} />` component (handles single- and multi-element shapes via per-entry stroke-width / fill overrides). Activity-bar contract extended with `iconName?: IconName` (wins over the legacy `iconPath` single-path string). Migrated `nexus.workflow` end-to-end as the proof; remaining plugins port lazily as they're touched.
- **Layout widths** — done in `04f2b7f`: sidebar default 220, rightPanel default 240 (BUILTIN_LAYOUTS.default + initial state in [layoutStore](../shell/src/stores/layoutStore.ts)), `.shell-statusbar` 24px → 32px in [shell.css](../shell/src/shell/shell.css). Persist version not bumped — existing user layouts keep their resized widths via shallow merge; new users land on the design bundle defaults.
- **Titlebar cluster** — done in `816de24`: left cluster (folder = open workspace · search = focus search), centred breadcrumb (sync dot · workspace name · active file · `ext · Nw` mono badge), right cluster (right-panel toggle), then existing min/max/close. All driven from real stores (`workspaceStore.rootPath`, `editorStore.tabs/activeRelpath`, `layoutStore.rightPanel.visible`); no hardcoded labels. Star + tweaks + dedicated backlinks toggle deferred — no real source for "starred files" yet, settings UI doesn't exist, and backlinks already lives as a right-panel tab.

---

## Phase 4 — Retire the template, delete `crates/nexus-app` · **Delivered**

- Delete `shell/src/plugins/core/*.tsx` once `nexus.*` counterparts replace them (kept during transition as reference).
- Delete `shell/src-tauri`'s community-plugin scan if `nexus.plugins-mgmt` replaces it.
- Merge `feat/ui-shell-rebuild` to `main`.
- Delete `crates/nexus-app` per decision 3c — shipped under Phase 4 WI-37 on 2026-04-24.

---

## Risks / gotchas

- **Forge root before workspace.** Kernel wants `forge_root` at boot. Until a workspace is chosen only `path_exists` / dialog / launcher bridges work — every `api.kernel.*` call errors. Plugins that need kernel must gate on `workspace:opened`.
- **Event flood → webview.** Typing-driven events (editor, outline, graph) must not fire 60/sec through Tauri serializer. Bridge forwarders coalesce per ~16ms or let the JS plugin pass a rate hint.
- **Subscription cleanup.** JS unsubscribe aborts both the JS listener and the Rust forwarder task. `Map<id, JoinHandle>` on the bridge side; plugin `deactivate` iterates its subscriptions.
- **Async runtime ownership.** Tauri 2 bundles tokio. `nexus_bootstrap::build` is sync but returns a `Runtime` that expects to be driven on tokio. Use `tauri::async_runtime::block_on` at setup time, then the runtime shares Tauri's executor.
- **Schema drift.** Kernel handler IDs are `u32` inside `CorePlugin::dispatch` but the public surface (`ipc_call(plugin_id, command_id, …)`) is string-keyed via plugin manifests. Keep using strings from JS.
- **Handler coverage.** Not every current shell operation has a kernel counterpart in all edge cases — mapping each to `com.nexus.*` is discovered per plugin in Phase 2. Placeholder: fall back to a thin Tauri command until the kernel catches up, but flag in the commit.

---

## Delivered

Commit log on `feat/ui-shell-rebuild`, most-recent first. Shell-side theme fixes (`850ca2c`, `23f96f1`, `9d84f80`) are pre-Phase-0 housekeeping from earlier in the branch and not listed here.

### Phase 0 — bridge infra

- `21509d0` — `nexus.launcher` plugin + `shell-state.json` persistence (Obsidian-style recents/Open/Create).
- `2581e8e` — add `nexus-kernel` / `nexus-bootstrap` / `nexus-plugin-api` / `nexus-plugins` path deps; empty `KernelRuntime` Tauri managed state.
- `36d7672` — `init_forge` / `boot_kernel` / `shutdown_kernel` Tauri commands + window-close hook. Kernel boots under `com.nexus.cli` invoker identity (upstream `build_shell_runtime` follow-up).
- `3ee56c5` — `kernel_invoke` / `kernel_subscribe` / `kernel_unsubscribe` Tauri commands + `api.kernel` frontend surface (`invoke`, `on`, `available`). Sync `AtomicBool` backs `available()`; `PluginContext` is `Arc`-cloned out of the mutex before `ipc_call.await` so invokes don't serialise.
- `9323cd5` — `nexus.workspace` drives the full kernel lifecycle (`null → path`, `path → other`, `path → null`). Boot failures re-throw so the launcher doesn't persist a broken recent.
- `5e354b1` — hoist `:root` token blocks into `shell/index.html` inline `<style>`. Vite's JS-injected `shell.css` was dropping them on documentElement; structural rules stay in `shell.css`.

### Phase 1 — retire standalone commands

- `0fee389` + `ccc99c6` — `nexus.files` via `com.nexus.storage::list_dir`, subscribed to `com.nexus.storage.file_{created,modified,deleted,renamed}`. Standalone `read_dir` Tauri command removed. Tree keys on forge-relative paths; `isDir` wire field.
- `186a6b0` — `nexus.gitStatus` via `com.nexus.git::status`; single prefix subscription covers state/branch_changed/commit/dirty_changed; `git2` direct dep dropped (transitively supplied by `nexus-git`).

### Phase 2 — feature plugins

- `0a736a7` — `nexus.editor` v0: read file via kernel, raw `<pre>`, top strip, no tabs.
- `6a7ac5b` — markdown rendering via `marked` + `dompurify`; shared `renderMarkdown` helper later extracted.
- `ca1825c` — tab bar: multi-file, click-to-switch, per-tab loading/error, × close button, `Ctrl+W` keybinding + `nexus.editor.hasActiveTab` context key.
- `f497656` — per-tab preview/source toggle, dirty tracking, save via `com.nexus.storage::write_file`, `Ctrl+S`, close-while-dirty confirm.
- `efc0c90` — `nexus.commandPalette` overlay, fuzzy subsequence match, `Ctrl+Shift+P` (+ `Ctrl+P` alias).
- `32c7556` — `nexus.rightPanel` host + `nexus.outline` plugin. New `rightPanelContent` SlotId; tab registration via `rightPanel:registerTab` event. `Ctrl+Alt+R` toggles visibility.
- `cced076` — `nexus.search` via `com.nexus.storage::search` (Tantivy); activity-bar item + sidebar view; 150ms debounce + `requestId` race guard; `Ctrl+Shift+F` focus command with focuser-singleton pattern.
- `e5929b4` — `nexus.backlinks` right-panel tab via `com.nexus.storage::backlinks`. Kernel returns edge metadata only (no line/excerpt).
- `7414f15` — `nexus.graph` right-panel tab; parallel `outgoing_links` + `backlinks` call, merged by neighbour with incoming/outgoing/both tagging; radial SVG layout, no physics.
- `08d6a66` — `nexus.terminal` in panelArea, backed by `com.nexus.terminal::create_session/send_raw_input/pump`. xterm.js + xterm-addon-fit; output is 50ms pull-poll (no kernel event topic yet); no resize handler upstream. `Ctrl+\`` toggles.
- `4694bef` — `nexus.ai` sidebar chat v1. Stateless RAG via `com.nexus.ai::ask { question, limit? }`; `RagResponse { answer, sources, model }`. No streaming, no multi-turn. Extracts `renderMarkdown` to `shell/src/plugins/nexus/editor/markdownRender.ts` for reuse.
- `7715730` — outline→editor scroll. Outline emits `editor:scrollToHeading { index, line }`; editor's preview body does `querySelectorAll('h1..h6')[index].scrollIntoView`, source mode scrolls textarea to the line. Added `index` to `OutlineHeading`.
- `09e31da` — `api.commands.all()` joins `KeybindingRegistry.all()` by commandId, populating `CommandEntry.keybinding` so palette rows can render a chord pill.
- `4504667` — pane-mode infra. New `paneMode` SlotId; `usePaneModeStore { activeViewId, enter, exit }`; `nexus.paneMode` plugin owns `nexus.paneMode.enter/exit` commands, `nexus.paneMode.active` + `nexus.paneMode.activeViewId` context keys, Escape exit gated by `!nexus.commandPalette.visible`. App.tsx swaps the tri-pane body for the active pane-mode entry when set.
- `b168e9a` — `nexus.pluginsMgmt` overlay modal, `Ctrl+Shift+X`. Reads `pluginList` + `communityPluginManifests` services; community rows toggle via `set_plugin_enabled` with optimistic UI + rollback on error. `core: true` for internal service access.
- `1ef3d81` — `nexus.processes` pane-mode. Left column lists built-in + community plugins from shell services, terminal sessions from `com.nexus.terminal::list_sessions`, MCP servers from `com.nexus.mcp.host::list_servers`. Right column streams a rolling 500-event buffer from 9 topic prefixes; filter input, follow-to-bottom toggle, clear button. Activity-bar item (2×2 grid glyph) routes in/out of pane mode via the existing `activityBar:activeChanged` event.
- `1323e55` — outline scroll-spy. Editor's body wrapper (preview) and textarea (source) get a rAF-throttled scroll listener that publishes `editor:activeHeadingChanged { index }`. Outline plugin forwards into a new `activeIndex` field on `useOutlineStore`; `OutlineView` highlights the active row (accent fg + left bar + raised bg) and `scrollIntoView({ block: 'nearest' })` keeps it visible. Source-mode mapping uses `scrollTop / line-height` and reads heading lines back from `useOutlineStore` (cross-plugin store import, same pattern outline already uses against the editor store). Preview-mode mapping queries the live `<h1..h6>` set under `markdownBodyRef` against `scrollWrapRef`'s top edge with an 8px tolerance to avoid boundary flicker. `recompute()` resets `activeIndex` alongside `setHeadings` so a tab switch can't briefly show a stale highlight.
- `6c86e92` — `nexus.workflow` sidebar plugin. Activity-bar item (lightning-bolt glyph, priority 30) toggles a sidebar listing of `.workflow.toml` files via `com.nexus.workflow::list`. Each row shows name + description + trigger-type chip + step-count chip + an `inputs` chip when `[inputs]` is non-empty, plus a Run button that invokes `com.nexus.workflow::run { name }` with a 5-minute timeout (matches the `run` handler's blocking semantics — it returns one final `WorkflowRun`). Per-workflow run status (`idle/running/done/error`) lives on the store and renders as a colour-coded pill next to the Run button; failures surface both in the pill (with the message in a `title` tooltip) and as a notification. List refresh on `workspace:opened`; reset on `workspace:closed`; manual refresh button in the header. `nexus.workflow.refresh` and `nexus.workflow.show` commands registered for palette discoverability. Inputs-prompt UI is a follow-up — workflows declaring required inputs without defaults will surface their failure in the row's status pill.
- `04f2b7f` — Phase 3 layout-width polish: sidebar default 260 → 220, rightPanel 300 → 240 in `BUILTIN_LAYOUTS.default` + the initial Zustand state, `.shell-statusbar` 24px → 32px. Persist version intentionally NOT bumped — shallow merge means existing users keep any layout they've manually resized, while new users (and anyone applying the "default" layout) land on the design bundle defaults.
- `7bce519` — typed icon module at `shell/src/icons/`. Ports all 27 glyphs from `.design-bundle/project/forge_icons.jsx` plus `refresh` + `play` (already needed by the shell). Single `<Icon name="…" size={16} />` wrapper handles single- and multi-element shapes via per-entry `strokeWidth` / `filled` overrides. `ActivityBarItem` gains `iconName?: IconName` (wins over the legacy `iconPath` single-path string), so plugins can register icons with multiple `<path>`/`<circle>`/`<rect>` elements that the path-only contract couldn't represent. Migrated `nexus.workflow` end-to-end as the proof; remaining plugins port lazily as they're touched.
- `7d0ac8b` — `nexus.skills` sidebar plugin. Activity-bar item (book glyph from the icons module, priority 40) toggles a sidebar listing of `.skill.md` files via `com.nexus.skills::list`. Kernel returns the full Skill struct (frontmatter flattened by serde plus the body) so each row carries everything the inline expand panel needs and click→expand has no second IPC call. Collapsed row shows name + version pill + description; expanded row reveals chips for `tags` / `applicable_contexts` / `triggers`, the author byline, and a fenced preview of the first 40 body lines (max-height 240px scroll panel). Same load-on-`workspace:opened` / clear-on-`workspace:closed` lifecycle as `nexus.workflow`, with the same `kernel.available()` probe to cover the first-boot race.
- `f7ad739` — `nexus.mcp` sidebar plugin. Activity-bar item (plug glyph, priority 50) toggles a sidebar listing of MCP servers from `com.nexus.mcp.host::list_servers`. Each row collapses to name + command + a status pill (`disabled` / `idle` / `connecting` / `up` / `disconnecting` / `error`); the kernel doesn't emit MCP topic events, so status is tracked locally based on the outcome of the shell's own invokes. Click-to-expand fires `list_tools` + `list_resources` + `list_prompts` in parallel against the server (kernel auto-connects on first call via `get_or_connect`); results cache so a second expand is instant. Inline Connect / Disconnect buttons let the user warm up or tear down a server without expanding. 60-second timeout on connect/list calls accommodates cold subprocess spawn. Tool-call args modal is a follow-up — the kernel handler `call_tool { server, tool, arguments }` is ready when the UI is.
- `816de24` — `nexus.titleBar` rewrite. Bare workspace-name button → design-bundle shape: left cluster (folder = open workspace · search = focus search), centred breadcrumb (sync dot · workspace name · active file · `ext · Nw` mono badge), right cluster with right-panel show/hide, then existing min/max/close. All driven from real stores (`workspaceStore.rootPath`, `editorStore.tabs/activeRelpath`, `layoutStore.rightPanel.visible`); no hardcoded labels. Star + tweaks + dedicated backlinks toggle deferred (no real "starred files" source, no settings UI yet, backlinks already a right-panel tab). Adds a small `runtime.ts` PluginAPI singleton so the React component can dispatch `nexus.search.focus` via `api.commands.execute`.
- `dbe5975` — settings panel wired in. `core.settings` was already implemented (overlay view + auto-generated UI from registered schemas + plugins-tab) but never registered in main.tsx, so the existing `Ctrl+,` keybinding fell on the floor. Registering it lights up the panel; plus `nexus.editor` contributes the first real schema: `confirmCloseDirty` (boolean, default true) and `defaultMode` (select preview/source, default preview). Editor reads both at runtime via `api.configuration.getValue` — `confirmCloseDirty=false` skips the close-while-dirty prompt, `defaultMode=source` flips new tabs into source mode at openTab time. Persists across restarts via the existing `shell-config` zustand store. Other plugins migrate as they need user-tunable behaviour.
- `4a14c00` — `nexus.confirm` plugin: shared overlay-slot dialog that replaces `window.confirm`. `api.input.confirm` in [host/PluginAPI.ts](../shell/src/host/PluginAPI.ts) lazy-imports `requestConfirm` from the new store so plugins keep calling `api.input.confirm(...)` without knowing this exists. Multiple concurrent confirm calls serialise via a FIFO queue. Esc / backdrop click → cancel; Enter → confirm; the confirm button autofocuses on mount. Migrated the editor's close-while-dirty prompt and the agent's history-delete prompt away from the platform popup; `window.confirm` now has zero callers in `shell/src/plugins/`. `api.input.prompt` is unchanged for now.
- `3223337` — `nexus.agent` step-by-step approval mode. Composer gains an Auto/Step pill toggle next to Run. In Step mode the Run flow peels apart into `plan` first, then a shell-driven loop: each step's row is highlighted with an accent border + Approve/Skip/Stop cluster while it awaits the user's call. Approve dispatches `com.nexus.agent::execute_step { plan, index }` and advances; Skip marks the step skipped without IPC; Stop marks remaining queued steps skipped and finishes. A failed `execute_step` short-circuits like the kernel's `run` would. Caveat: `execute_step` does NOT persist to history (the kernel handler bypasses save_history), so stepped runs don't appear in the left-column list — flagged in the agentStore type comment. Auto mode still routes through `run` and persists as before.
- `49b961e` — `nexus.agent` history-delete + `trash` icon. Trash button on each history row (also added `trash` to the icon module — Lucide-style). Click → `window.confirm` (matches the close-while-dirty stop-gap) → `com.nexus.agent::history_delete { plan_id }`. If the deleted run was loaded into the right column, the plan/observation/phase reset so we're not pointing at a vanished record. Failures surface via notifications.
- `d677d01` — `nexus.mcp` tool-call modal. Each tool row in the expanded server panel gains a Call button; click opens an overlay-slot modal with the `serverName · toolName` in the title and a JSON textarea for arguments (defaults to `{}`). Run parses + validates the JSON object (rejects arrays / scalars), then invokes `com.nexus.mcp.host::call_tool { server, tool, arguments }`. Result panel renders the kernel's `{ content[], is_error }` — `text` content variants get a prose pre-block, anything else gets a JSON dump with a 320px max-height scroll. A successful call also flips the row's status pill to `up` so the user doesn't need an explicit reconnect dance to reflect "live". Esc / backdrop click / Cancel close the modal (gated while a run is in flight). Inputs-schema-driven form is a follow-up if/when the kernel's `list_tools` starts surfacing `inputSchema`.
- `29563bc` — `nexus.agent` paneMode plugin. Activity-bar item (sparkle glyph, priority 70) routes the shell into a full-pane workspace with a 240px history column on the left and a goal composer + plan view + observation on the right. **Plan** button calls `com.nexus.agent::plan { goal }` and renders the returned `Plan` (numbered steps with descriptions + tool-call summaries). **Run** button calls `com.nexus.agent::run { goal }` which plans + executes server-side; per-step status (`queued`/`running`/`ok`/`failed`/`skipped`) updates live via the `com.nexus.agent.{run_start,step_start,step_done,run_done}` topic stream the kernel publishes during the run. Once the awaited `Observation` lands, the shell calls `history_get { plan_id }` to backfill the `Plan` (the topic stream describes step ids without their full bodies), then refreshes the history list so the row appears on the left. Click any history row → load that plan + observation into the right column. 5-minute bridge timeout accommodates LLM-bound runs. Per-step approval (`HANDLER_EXECUTE_STEP`), archetype picker, and history-delete UI are intentionally out of v1 — handlers are ready when the UI lands.
