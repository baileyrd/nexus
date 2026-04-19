# Shell ↔ Kernel Bridge — Implementation Plan

Living document. Each phase commit updates the relevant section in-place; finished phases move to the _Delivered_ section at the bottom.

## Status

Branch: `feat/ui-shell-rebuild`. Shell plugin system is in place and rendering real data for workspace / gitStatus / titleBar / activityBar / sidebar / files via standalone Tauri commands (std::fs + git2). Kernel is NOT yet wired in. This document covers the plan to bridge the shell to the existing Nexus kernel (`crates/nexus-*`) while preserving the React plugin architecture.

## Decisions

| # | Question | Answer |
|---|---|---|
| 1 | First launch UX | Obsidian-style launcher modal (recents list + Create / Open / Clone actions). Shell boots with no kernel; kernel boots on workspace pick. |
| 2 | Storage backend | **Option D — match `crates/nexus-app`**: single JSON file at `<app_config_dir>/shell-state.json`, read once at startup, atomic write (tmp → rename). Per-forge UI state keyed by absolute path. Kernel KV stays for kernel-plugin state only. localStorage + zustand-persist phased out as plugins migrate. |
| 3 | Fate of `crates/nexus-app` | **3c — keep during transition, delete at parity.** No user disruption; shell proves itself before cutover. |
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

## Phase 0 — Bridge infrastructure

Foundations before any kernel-backed feature can land.

**Steps:**

1. **Persistence** — port [crates/nexus-app/src/persistence.rs](../crates/nexus-app/src/persistence.rs) into `shell/src-tauri/src/persistence.rs`. Expose Tauri commands `get_shell_state()`, `save_shell_state(state)`, `write_last_forge_path(path)`, `read_last_forge_path()`. Shell-side wrapper: load once at startup into a Zustand store; writes debounced ~500ms to `save_shell_state`.
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

## Phase 1 — Retire standalone commands, route through kernel

Mechanical replacement. Each plugin flips one `invoke('…')` to `api.kernel.invoke('com.nexus.*', '…')`.

- `read_dir` → `com.nexus.storage::list_dir`. Delete `read_dir` from [shell/src-tauri/src/lib.rs](../shell/src-tauri/src/lib.rs).
- `get_git_status` → `com.nexus.git::status`. Delete the `git2` dep and standalone command.
- `path_exists` stays — trivial, used pre-kernel for workspace verification.
- `nexus.files` subscribes to `com.nexus.storage.file_created / file_modified / file_deleted` for live refresh instead of manual refetch.
- `nexus.gitStatus` subscribes to `com.nexus.git.branch_changed / commit / dirty_changed` for live updates.

**Acceptance:** same observable behaviour with standalone commands removed from lib.rs. File-tree + git status update live as files change / branches move.

---

## Phase 2 — Feature plugins (per design bundle layout)

Each is a self-contained plugin. Order by dependency + value.

| # | Plugin | Kernel-side | Slots | Design ref |
|---|---|---|---|---|
| 1 | `nexus.editor` | `com.nexus.editor::open/save/apply_transaction` | `editorArea` + `editorTabs` | [forge_doc.jsx](../.design-bundle/project/forge_doc.jsx) |
| 2 | `nexus.outline` | editor content + local parser | `rightPanel` (Outline tab) | [forge_panels.jsx](../.design-bundle/project/forge_panels.jsx) |
| 3 | `nexus.search` | `com.nexus.storage::search` (Tantivy) | `activityBar`, `sidebarContent` | forge_panels |
| 4 | `nexus.backlinks` | `com.nexus.storage::query_backlinks` | `rightPanel` (Backlinks tab) | forge_panels |
| 5 | `nexus.graph` | `com.nexus.storage::query_graph` | `rightPanel` (Graph tab) | forge_panels |
| 6 | `nexus.commandPalette` | aggregated commands | `overlay` (⌘P) | Forge PALETTE data |
| 7 | `nexus.terminal` | `com.nexus.terminal::create_session/send_input/pump` | `panelArea` | [forge_processes.jsx](../.design-bundle/project/forge_processes.jsx) |
| 8 | `nexus.processes` | kernel plugin status + pump | full-pane mode | forge_processes |
| 9 | `nexus.tasks` | `com.nexus.storage::task_query` | `activityBar`, `sidebarContent` | — |
| 10 | `nexus.ai` | `com.nexus.ai::stream_chat/session_*` | full-pane mode | [forge_orchestrate.jsx](../.design-bundle/project/forge_orchestrate.jsx) (partial) |
| 11 | `nexus.agent` | `com.nexus.agent::plan/run` | full-pane mode | forge_orchestrate |
| 12 | `nexus.workflow` | `com.nexus.workflow::list/run` | full-pane mode | — |
| 13 | `nexus.templates` | kernel read | full-pane mode | [forge_templates.jsx](../.design-bundle/project/forge_templates.jsx) |
| 14 | `nexus.skills` / `nexus.mcp` / `nexus.settings` / `nexus.plugins-mgmt` | respective kernel plugins | various | — |

Pane-mode plugins (terminal/processes, ai/agent, templates) mirror the design's `pane === 'ai' | 'terminal' | 'templates'` flag — when their activity icon is active the tri-pane is replaced with a full-window workspace view.

**Acceptance per plugin:** observable feature parity with `crates/nexus-app`'s equivalent panel, end-to-end data real, no hardcoded content, tokens via CSS vars.

---

## Phase 3 — Visual polish

Already done: `shell.css :root` carries the exact oklch palette from the design bundle. Memory `feedback_design_bundle_source_of_truth.md` enforces token-only usage.

Remaining:
- **Icon module** `shell/src/icons/` — port SVG path constants from [forge_icons.jsx](../.design-bundle/project/forge_icons.jsx) into a typed TS map. Replace ad-hoc `<svg>` definitions scattered across plugins.
- **Layout widths** — activity 44 (done), sidebar default 220 (currently 260), rightPanel default 240 (currently 300), statusbar 32 (currently 24). One-line tweaks in [layoutStore](../shell/src/stores/layoutStore.ts).
- **Titlebar cluster** — left-side icon cluster (folder / search / star) + breadcrumb with workspace · file · type badge, as Phase 2 plugins progressively contribute.

---

## Phase 4 — Retire the template, delete `crates/nexus-app`

- Delete `shell/src/plugins/core/*.tsx` once `nexus.*` counterparts replace them (kept during transition as reference).
- Delete `shell/src-tauri`'s community-plugin scan if `nexus.plugins-mgmt` replaces it.
- Merge `feat/ui-shell-rebuild` to `main`.
- Delete `crates/nexus-app` per decision 3c.

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

_Nothing yet — Phase 0 scaffolding in progress._
