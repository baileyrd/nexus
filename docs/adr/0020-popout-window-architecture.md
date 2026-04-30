# ADR 0020: Popout Window Architecture (BL-029 Phase 2)

**Date:** 2026-04-30
**Status:** Accepted

## Context

BL-029 Phase 1 (shipped 2026-04-29) landed the Tauri-side primitives —
`popout_window` / `close_popout_window` / `list_popout_windows` /
`get_popout_window_bounds` / `set_popout_window_bounds` — plus the
workspace-store mutations (`popoutLeaf`, `closeFloatingWindow`,
`setFloatingWindowBounds`) that move a leaf out of its parent `Tabs`
into a `FloatingWindow` node persisted in `<forge>/.forge/workspace.json`.
A child webview is opened with `index.html?popout=<fwId>&leaf=<leafId>`.

What Phase 1 did **not** ship is the actual rendering of the popped-out
leaf inside the child webview. Today `PopoutShell.tsx` is a placeholder
explaining that "Detached panel rendering will land in BL-029 Phase 2."

The Phase-2 follow-up entry in [BACKLOG.md](../PRDs/BACKLOG.md) enumerates
four open design questions that have to be resolved before code:

1. **Plugin boot scope.** Each popout is its own JS context. What plugin
   set should it activate, and how is it kept in sync with the main
   window's set?
2. **Cross-window sync semantics.** File edits in a popout need to
   invalidate the main window's preview; the kernel handles the
   file-watcher event but the shell-local `eventBus` is per-window.
3. **Close-while-editing rescue.** If the popout has unsaved buffer
   state and the user closes via the OS-X button, what happens?
4. **Stale-leaf reconciliation on main-window reload.** The Tauri-side
   popout windows can outlive a main-window reload; persisted
   `floating[]` can carry leaf ids that no longer resolve.

This ADR records the Phase-2 design decisions for those four questions
and a handful of derived choices.

## Decisions

### 1. Plugin boot scope — full parity, minus marketplace surface

**A popout webview boots the same `DEFAULT_ON` plugin set that the main
window boots, plus the same opt-in plugins from `plugins.enabled`.**

A trimmed "minimal" set (e.g. just the editor and the file explorer)
was rejected because:

- Determining a minimal set requires a static dependency graph the
  shell does not have. Most contributing plugins announce themselves
  through `viewRegistry.register` at activation time, and a Leaf can
  carry any `viewType` — restricting the popout's plugin set means a
  popped-out canvas, bases, terminal, or graph leaf would silently
  fall through to the `empty` view creator.
- The cost is small. Most plugins are lazy-activated via
  `activationEvents`. A second `host.loadAll(DEFAULT_ON_PLUGINS)` adds
  ~100 ms to popout open on a typical machine and zero steady-state
  cost.
- Plugin activation is idempotent on a fresh JS context. There is no
  shared state between the main window's `PluginRegistry` and the
  popout's, so re-running activation in the popout is a clean
  registration into a fresh registry.

**Three subsystems are intentionally skipped in popout-mode boot:**

- **Community plugins (`scanCommunityPlugins`,
  `loadEnabledCommunityPlugins`, sandbox orchestrator).** Until the
  marketplace UI lands, community plugins are first-party-only and
  primarily contribute to the main-window chrome. Skipping them in
  popouts avoids double-running the install-time consent prompt and
  the iframe sandbox bootstrap, which are non-trivial cost.
- **Install-time consent (`runInstallTimeConsent`).** Bound to the
  community-plugin set above; pointless without it.
- **Workspace auto-save (`installAutoSave`).** Two windows writing the
  same `workspace.json` would race. The main window remains the sole
  writer; popouts read the file at boot to locate their leaf, then
  treat their copy as read-only. Layout mutations performed inside a
  popout (none today — popouts are single-leaf) would not persist;
  Phase-3 multi-leaf popouts will need a different sync model.

**Plugins that do contribute main-window chrome (activity bar, status
bar, ribbon, title bar) still activate in popout-mode**, but their
contributions go into the popout's own SlotRegistry — which the popout
shell does not render. This is a small amount of dead work in exchange
for a simple, uniform boot path. A plugin that needs to skip an
expensive contribution in popout-mode can read
`contextKeyService.snapshot()['popoutMode'] === true`, which the popout
boot path sets before activation.

### 2. Cross-window sync — through the shared kernel only

**All windows share the same `KernelRuntime` via Tauri managed state.
Cross-window state synchronization happens *exclusively* through kernel
events.** No window-to-window IPC, no Tauri-event broadcast layer, no
shared zustand store.

Concretely:

- **Editor sessions are kernel-side singletons** keyed by relpath
  (`crates/nexus-editor/src/core_plugin.rs`). Two windows opening the
  same file get the same `Session`. Every successful mutation
  publishes `com.nexus.editor.changed.<relpath>` on the kernel
  event bus. Each window's `transactionBridge` already subscribes via
  `kernel_subscribe`, so a popout editing a file already triggers a
  reconciliation in the main window's preview — no new wiring.
- **File watcher events** (`com.nexus.storage.file.*`) reach all
  windows the same way.
- **Per-window state** that does *not* need to sync — pane mode,
  context keys, slot registry, command palette state — stays local to
  each window's zustand stores. This is intentional: a popout's "active
  leaf" is *itself*, and the main window's "active leaf" is a separate
  concept.

There is one consequence to call out: **the workspace tree
(`workspaceStore`) is per-window**. The main window owns the
authoritative copy and persists it. A popout hydrates its own copy
read-only from `workspace.json` so it can locate its leaf, but layout
mutations in the popout do not propagate back. For single-leaf popouts
this is correct: there are no layout mutations to propagate.

### 3. Close-while-editing — close-as-tab-close, kernel session is the rescue

**Closing a popout window (whether via the in-shell control or the OS-X
button) removes the matching `FloatingWindow` from `floating[]` in the
main window's store. The leaf is fully closed.** This matches the
existing `closeFloatingWindow` semantics from Phase 1 — popouts are not
"detached docks that can be re-attached", they are tabs that happen to
live in their own OS window.

There is **no save-on-close confirmation dialog**. Instead, the editor's
existing dirty-flush pipeline handles the rescue:

- `EditorCorePlugin::HANDLER_SAVE` is debounced and called from the
  editor's transaction bridge after every successful mutation.
- When the popout webview unloads, `beforeunload` fires
  `host.deactivateAllForShutdown(1000)` (already wired in
  `main.tsx`), which gives every plugin a 1 s soft cap to flush state.
  The editor plugin's deactivate path issues a final synchronous save
  for any session whose dirty flag is still set.
- The kernel session itself outlives the popout's webview — it is
  owned by the kernel, not the JS context — so even if the dirty flush
  somehow misses, reopening the file in the main window will surface
  the same in-memory buffer.

Phase-3 may revisit this if user feedback demands a "Save before
closing this popout?" prompt, but the default Obsidian-style "edits are
flushed continuously, close is cheap" posture is the starting point.

**Implementation:** the popout-mode shell binds
`tauri::Window::on_close_requested` (frontend-side via
`getCurrentWindow().onCloseRequested`) and emits a Tauri-app event
(`nexus:popout-closed`) carrying the `fwId` to the main window before
the popout webview tears down. The main window listens for that event
and dispatches `workspace.closeFloatingWindow(fwId)`, removing the
entry from `floating[]` and triggering autosave.

### 4. Stale-leaf reconciliation — popout fails closed; main is authoritative

**The main window is the authoritative source of `floating[]` state.
Popouts hydrate read-only and fail closed if their fwId/leafId no
longer resolves.**

Three concrete edges:

- **Popout boots, fwId not in `workspace.json`.** Render an error
  state ("This popout's window state is out of sync. Close to
  continue.") and stop. Do not auto-close — the user might want to
  copy-paste content out before dismissing.
- **Popout boots, fwId resolves but the contained leaf has no view
  state (or its `viewType` has no registered creator).** Render the
  same error state. This indicates the leaf was deleted out from
  under the popout (e.g. `workspace.json` was hand-edited or a
  migration ran) or the contributing plugin was disabled.
- **Main window reloads while popouts are alive.** Existing
  `restoreFloatingWindows()` reconciliation in
  [popoutWindowBridge.ts](../../shell/src/workspace/popoutWindowBridge.ts)
  already handles this: the store-side `floating[]` is hydrated from
  `workspace.json`, then reconciled against `list_popout_windows()`.
  Popouts the store knows about but Tauri doesn't are reopened;
  Tauri-side popouts the store doesn't know about are closed. This
  ADR adds no new behaviour here.

There is a deliberate asymmetry: the main window can recover by closing
unwanted popouts, but a popout cannot recover by re-opening itself
into the main window's tree. That asymmetry is fine because the user
always has the main window in front of them when this race surfaces.

## Consequences

### Positive

- One boot path for the whole app — popouts run the same plugin
  activation code as the main window. No "popout-only" plugin
  manifests, no minimal-set list to maintain.
- Cross-window sync is "free" — every plugin that already routes
  through `ipc_call` and `kernel_subscribe` works in popouts on the
  first try.
- The persistence story is dead simple: one writer (main window),
  hydrate-on-boot for everyone else.

### Negative / accepted trade-offs

- ~100 ms popout open latency from the second plugin activation. Not
  visible to the user during the OS-side window-open animation.
- A small amount of dead chrome registration in popouts (activity bar,
  status bar, ribbon contributions that the popout doesn't render).
  Plugins that want to skip these can read the `popoutMode` context
  key.
- No layout mutation in popouts (no tab strip, no split). Acceptable
  for single-leaf popouts; revisit if multi-leaf popouts ship.
- No save-on-close prompt. Relies on the editor's continuous dirty
  flush. Documented above.

### Open follow-ups (Phase 3, not gating Phase 2)

- Multi-leaf popouts (a popout that hosts a `Tabs` of multiple leaves,
  with its own tab strip).
- Drag-back affordance — a popout title-bar control that re-attaches
  the leaf into the main window's currently-active tab group.
- Popout-side chrome contributions (status bar at minimum, so users
  see their forge name and the active relpath without alt-tabbing).

## Cross-references

- [BACKLOG.md — BL-029](../PRDs/BACKLOG.md)
- [PRD-17](../PRDs/) — multi-window requirements
- [shell/src-tauri/src/windows.rs](../../shell/src-tauri/src/windows.rs) — Phase 1 Tauri primitives
- [shell/src/workspace/popoutWindowBridge.ts](../../shell/src/workspace/popoutWindowBridge.ts) — workspace-store ↔ Tauri bridge
