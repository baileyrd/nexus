# Hardcoded Settings Audit

> **Inventory only.** Live extractions are tracked in [../docs/PRDs/BACKLOG.md → "Settings extraction queue"](../docs/PRDs/BACKLOG.md#settings-extraction-queue). This file is the source-of-truth catalogue of named-constant candidates; pick from BACKLOG.md when scheduling work.

**Date:** 2026-04-25 (revised 2026-04-25)
**Scope:** `shell/src/plugins/` and `shell/src/shell/`

Values are split into two tracks:

- **User Config** — should appear in the settings UI; an end user would reasonably want to change these.
- **Dev Config** — implementation/performance constants; a developer tunes these, not a user. These don't belong in the settings UI but should be named constants (or env/build-time overrides) rather than inline magic numbers.

---

## User Config

Things that belong in the settings panel.

### Keybindings

All shortcuts are declared in plugin `index.ts` manifests — already the right structure for a keybinding schema. The registry already supports overrides via `bindStorage`/`setOverride`. The remaining work is exposing defaults as configurable.

| File | Line | Shortcut | Suggested Key |
|------|------|----------|---------------|
| `core/settings/index.ts` | 39–40 | `ctrl+,` / `cmd+,` | `keybindings.openSettings` |
| `core/zoom/index.ts` | 43–49 | `ctrl+=/+` / `cmd+=` | `keybindings.zoom*` |
| `core/rightPanel/index.ts` | 25 | `ctrl+alt+b` / `cmd+alt+b` | `keybindings.rightPanelToggle` |
| `core/editorArea/index.ts` | 23–25 | `ctrl+w`, `ctrl+tab`, `ctrl+shift+tab` | `keybindings.editorClose/Next/Prev` |
| `core/terminal/index.ts` | 25 | `ctrl+\`` / `cmd+\`` | `keybindings.terminalToggle` |
| `core/panelArea/index.ts` | 26 | `ctrl+j` / `cmd+j` | `keybindings.panelToggle` |
| `core/commandPalette/index.ts` | 27, 34 | `ctrl+shift+p`, `ctrl+p`, etc. | `keybindings.commandPalette` |
| `nexus/pluginsMgmt/index.ts` | 244 | `ctrl+shift+x` / `cmd+shift+x` | `keybindings.pluginsOpen` |
| `nexus/search/index.ts` | 42 | `ctrl+shift+f` / `cmd+shift+f` | `keybindings.searchFocus` |
| `nexus/workspace/index.ts` | 44 | `ctrl+o` | `keybindings.workspaceOpen` |
| `nexus/ai/index.ts` | 71 | `ctrl+alt+a` / `cmd+alt+a` | `keybindings.aiFocus` |
| `nexus/editor/index.ts` | 111–117 | `ctrl+w/s` / `cmd+w/s` | `keybindings.editorSave/Close` |
| `nexus/canvas/index.ts` | 58–65 | `ctrl+z/y`, `shift+f`, `shift+/` | `keybindings.canvas*` |
| `nexus/rightPanel/index.ts` | 33 | `ctrl+alt+r` / `cmd+alt+r` | `keybindings.rightPanelToggleAlt` |
| `nexus/terminal/index.ts` | 84 | `ctrl+\`` / `cmd+\`` | `keybindings.terminalToggleAlt` |
| `nexus/fileExplorer/index.ts` | 20 | `ctrl+k ctrl+o` / `cmd+k cmd+o` | `keybindings.fileExplorerOpen` |

### Zoom

`core/zoom` already has named constants — needs a settings schema wired up.

| File | Line | Value | Suggested Key |
|------|-------|-------|---------------|
| `core/zoom/index.ts` | 15 | `0.1` step | `ui.zoomStep` |
| `core/zoom/index.ts` | 16 | `0.5` min | `ui.zoomMin` |
| `core/zoom/index.ts` | 17 | `3.0` max | `ui.zoomMax` |
| `core/zoom/index.ts` | 18 | `1.0` default | `ui.zoomDefault` |

### Terminal Appearance

`fontSize` and `fontFamily` already have schemas — parity items for completeness.

| File | Line | Value | Suggested Key |
|------|------|-------|---------------|
| `core/terminal/index.ts` | 33 | `13` px | `terminal.fontSize` *(done)* |
| `core/terminal/index.ts` | 40 | `'monospace'` | `terminal.fontFamily` *(done)* |
| `nexus/terminal/savedCommandsStore.ts` | 85 | `2_000` ms | `terminal.autoRestartDelayMs` |

### Canvas

| File | Line | Value | Suggested Key |
|------|------|-------|---------------|
| `nexus/canvas/renderer.ts` | 32 | `250 × 60` px | `canvas.defaultTextNodeSize` |
| `nexus/canvas/exportFormats.ts` | 18–19 | `48` units, `8192` px | `canvas.exportMarginUnits` / `canvas.maxExportEdgePx` |
| `nexus/canvas/exportPng.ts` | 17, 20 | `48` px, `8192` px | `canvas.exportMarginPx` / `canvas.maxExportEdge` |
| `nexus/canvas/Inspector.tsx` | 346 | Color swatches array | `canvas.colorSwatches` |

### Search & Command Palette

| File | Line | Value | Suggested Key |
|------|------|-------|---------------|
| `nexus/search/searchRuntime.ts` | 19 | `50` results | `search.maxResultsLimit` |
| `nexus/commandPalette/match.ts` | 9 | `50` commands | `commandPalette.maxResultsLimit` |

### Notification Durations

| File | Line | Value | Suggested Key |
|------|------|-------|---------------|
| `core/notificationService/index.ts` | 26 | `4000` ms | `ui.notificationDurationMs` |
| `core/fileExplorer/index.ts` | 61 | `2000` ms | `ui.fileCreationNotificationMs` |
| `nexus/terminal/SavedCommandsView.tsx` | 122 | `3000` ms | `ui.commandSaveNotificationMs` |
| `nexus/terminal/SavedCommandsView.tsx` | 135 | `1800` ms | `ui.commandCopiedNotificationMs` |
| `nexus/ai/ChatView.tsx` | 478 | `1200` ms | `ui.copiedNotificationMs` |

### Layout Density

Likely deferred until a density/theming system is designed — CSS variables may be the right layer rather than settings keys.

| File | Line | Value | Suggested Key |
|------|------|-------|---------------|
| `nexus/bases/BasesTable.tsx` | 319 | `28` px | `bases.tableRowHeight` |
| `nexus/bases/BasesTimeline.tsx` | 250–251 | `32` px, `140` px | `bases.timelineLaneHeight` / `bases.timelineGroupLabelWidth` |
| `nexus/outline/OutlineView.tsx` | 90 | `18` px | `outline.indentPx` |
| `nexus/files/FilesTree.tsx` | 18 | `14` px | `files.indentPx` |

---

## Dev Config

Implementation/performance constants. Not user-facing — should be named constants or env/build-time overrides, not inline magic numbers.

### Operation Timeouts

| File | Line | Value | Constant Name |
|------|------|-------|---------------|
| `nexus/ai/aiRuntime.ts` | 55 | `60_000` ms | `AI_REQUEST_TIMEOUT_MS` |
| `nexus/agent/index.ts` | 52 | `5 * 60_000` ms | `AGENT_RUN_TIMEOUT_MS` |
| `nexus/workflow/index.ts` | 30 | `5_000` ms | `WORKFLOW_VALIDATE_TIMEOUT_MS` |
| `nexus/workflow/index.ts` | 35 | `5 * 60_000` ms | `WORKFLOW_RUN_TIMEOUT_MS` |
| `nexus/mcp/index.ts` | 44 | `60_000` ms | `MCP_CONNECT_TIMEOUT_MS` |
| `nexus/terminal/TerminalView.tsx` | 32 | `5000` ms | `PTY_POLL_INTERVAL_MS` |
| `nexus/terminal/TerminalView.tsx` | 37 | `30` ms | `PTY_PUMP_TIMEOUT_MS` |
| `nexus/terminal/index.ts` | 34 | `250` ms | `TERMINAL_RECOVERY_TIMEOUT_MS` |

Note: AI, agent, and workflow all independently use `5 * 60_000` ms — good candidate for a shared `LONG_RUNNING_OP_TIMEOUT_MS` base constant.

### Debounce & Poll Intervals

| File | Line | Value | Constant Name |
|------|------|-------|---------------|
| `nexus/ai/aiRuntime.ts` | 355 | `1000` ms | `AUTOSAVE_DEBOUNCE_MS` |
| `nexus/search/searchRuntime.ts` | 76 | `150` ms | `SEARCH_DEBOUNCE_MS` |
| `nexus/canvas/patchQueue.ts` | 69 | `250` ms | `PATCH_DEBOUNCE_MS` |
| `nexus/canvas/CanvasOverlay.tsx` | 74 | `250` ms | `TERMINAL_NODE_POLL_MS` |
| `core/capabilityPrompt/CapabilityBannerView.tsx` | 13 | `10_000` ms | `BANNER_AUTO_DISMISS_MS` |

### Buffer & Event Caps

| File | Line | Value | Constant Name |
|------|------|-------|---------------|
| `nexus/canvas/CanvasOverlay.tsx` | 69 | `32 * 1024` bytes | `TERMINAL_NODE_BUFFER_CAP` |
| `nexus/canvas/CanvasOverlay.tsx` | 588 | `64 * 1024` bytes | `FILE_PREVIEW_TEXT_CAP` |
| `nexus/processes/processesStore.ts` | 45 | `500` events | `PROCESS_EVENTS_CAP` |
| `nexus/bases/basesStore.ts` | 89 | `200` entries | `BASES_HISTORY_CAP` |
| `nexus/canvas/canvasStore.ts` | 36 | `200` entries | `CANVAS_HISTORY_CAP` |

Note: canvas and bases both use `200` — consider a shared `UNDO_HISTORY_CAP`.

### Physics & Rendering

| File | Line | Value | Constant Name |
|------|------|-------|---------------|
| `nexus/graph/forceLayout.ts` | 34 | `0.82` | `DAMPING_FACTOR` |
| `nexus/graph/forceLayout.ts` | 35 | `40` | `MAX_VELOCITY` |
| `nexus/graph/forceLayout.ts` | 37 | `600` nodes | `PAIRWISE_FULL_LIMIT` |
| `nexus/canvas/autoLayout.ts` | 56 | `250` iterations | `AUTO_LAYOUT_ITERATIONS` |
| `nexus/canvas/renderer.ts` | 361 | `16` samples | `EDGE_HIT_TEST_SAMPLES` |

### Geometry Constants

Hit-test radii and handle sizes — belong in the renderer as named constants, not inline numbers.

| File | Line | Value | Constant Name |
|------|------|-------|---------------|
| `nexus/canvas/renderer.ts` | 36 | `40` px | `MIN_NODE_SIZE` |
| `nexus/canvas/renderer.ts` | 40 | `5` px | `HANDLE_HALF_SIZE` |
| `nexus/canvas/renderer.ts` | 292 | `14` px | `EDGE_HANDLE_OFFSET` |
| `nexus/canvas/renderer.ts` | 294 | `7` px | `EDGE_HANDLE_RADIUS` |
| `nexus/canvas/Minimap.tsx` | 22–23 | `200 × 140` px | `MINIMAP_WIDTH` / `MINIMAP_HEIGHT` |
| `nexus/canvas/Minimap.tsx` | 27 | `8` px | `MINIMAP_PADDING` |
| `nexus/graph/GraphView.tsx` | 9–10 | `240 × 300` px | `GRAPH_DEFAULT_WIDTH` / `GRAPH_DEFAULT_HEIGHT` |
| `nexus/launcher/LauncherView.tsx` | 15 | `36` px | `TITLEBAR_HEIGHT` |
| `nexus/bases/basesStore.ts` | 272 | clamp `[2, 80]` | `TIMELINE_DAY_PX_MIN` / `TIMELINE_DAY_PX_MAX` |

### Miscellaneous

| File | Line | Value | Constant Name |
|------|------|-------|---------------|
| `nexus/ai/aiRuntime.ts` | 51 | `5` results | `RAG_TOP_K` |
| `nexus/ai/aiRuntime.ts` | 358 | `48` chars | `AI_TITLE_MAX_CHARS` |
| `nexus/graph/GraphView.tsx` | 14 | `14` chars | `GRAPH_LABEL_MAX_CHARS` |
| `nexus/canvas/renderer.ts` | 21–27 | `#1e1e1e`, etc. | CSS token fallbacks — use real theme tokens |
| `nexus/canvas/Minimap.tsx` | 77–79 | `#1e1e1e`, etc. | CSS token fallbacks — use real theme tokens |
| `core/settings/index.ts` | 76 | GitHub URL | `HELP_URL` constant |

---

## Recommended Order

### User Config (settings UI)
1. **Zoom** — named constants already exist, just needs the schema wired up. Fastest win.
2. **Notification durations** — simple schema additions, low risk.
3. **Search/palette limits** — one schema key each, trivial.
4. **Terminal auto-restart delay** — additive to existing terminal schema.
5. **Canvas export settings** — export margin and max edge are the most user-impactful.
6. **Canvas color swatches** — quality-of-life for canvas users.
7. **Keybindings** — already configurable via overrides UI; manifest defaults are the remaining gap.
8. **Layout density** — defer; CSS variables may be the better mechanism.

### Dev Config (named constants)
1. **Timeout consolidation** — define `LONG_RUNNING_OP_TIMEOUT_MS` and replace the three independent `5 * 60_000` literals.
2. **Buffer/event caps** — name all five cap constants; consider shared `UNDO_HISTORY_CAP`.
3. **Canvas geometry** — name the renderer hit-test and handle constants.
4. **CSS token fallbacks** — replace hardcoded hex in renderer and minimap with real theme tokens (same pattern as the keybindings UI fix).
