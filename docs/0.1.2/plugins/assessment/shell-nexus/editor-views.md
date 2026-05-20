# Editor and Alternate Views

This category covers the markdown editor itself plus alternate editing /
layout surfaces that mount their own view types in the workspace. `nexus.editor`
is the real CodeMirror 6 editing experience; the shell-core
`core.editor-area` plugin only owns tab-strip primitives (closeTab, nextTab,
etc.) and is not a renderer. `nexus.multibufferSync` is an editor-coupled
helper that keeps multibuffer excerpts fresh. `nexus.paneMode`, `nexus.canvas`,
`nexus.notion`, and `nexus.viewBuilder` add discrete alternate surfaces on
top of the editor.

### nexus.editor

- **Path:** `shell/src/plugins/nexus/editor/`
- **Surface:** Registers the `markdown`, empty, and tab content views; CM6
  setup with slash menu, block handles, inline toolbar, margin
  suggestions, blame gutter, live preview, code mode, REPL kernels,
  diff view. Contributes ~25+ commands (`nexus.editor.save`,
  `nexus.editor.closeTab`, find/replace, toggle mode, copy rel/abs path,
  reveal in OS/nav, delete file, toggle blame, open diff, LSP rename /
  rename-preview / find-references / code-actions, plus stub split-right
  / open-linked-view placeholders).
- **Depends on:** `nexus.files` (filesStore, kernel handle), `nexus.workspace`,
  `nexus.comments` (`createCommentsApi`); backend `com.nexus.storage`
  (`read_file`, `write_file`), `com.nexus.editor` (session lifecycle),
  `com.nexus.git` (diff/blame), `com.nexus.lsp` (rename, references,
  code-actions).
- **Verdict:** Essential
- **Rationale:** This is the markdown editor. The shell-core
  `core.editor-area` plugin only manages the tab strip — without
  `nexus.editor` there is no way to open or modify a `.md` file.

### nexus.multibufferSync

- **Path:** `shell/src/plugins/nexus/multibufferSync/`
- **Surface:** No views or commands. Subscribes to `files:open` and to
  `com.nexus.editor.changed.<relpath>` kernel events; for every open
  `multibuffer://<uuid>` it tracks the set of source files in its excerpts
  and calls `com.nexus.editor::refresh_excerpts` whenever one changes.
- **Depends on:** `nexus.editor` (declared in `dependsOn`); backend
  `com.nexus.editor` session + excerpt APIs.
- **Verdict:** Useful
- **Rationale:** Required for multibuffer correctness (LSP find-references
  results, rename previews) but multibuffer itself is a feature surface —
  not required for opening and editing a single markdown file. If
  multibuffer were removed this plugin would be dead.

### nexus.paneMode

- **Path:** `shell/src/plugins/nexus/paneMode/`
- **Surface:** Commands `nexus.paneMode.enter` / `nexus.paneMode.exit`;
  keybinding Escape (when `nexus.paneMode.active && !nexus.commandPalette.visible`);
  context keys `nexus.paneMode.active` and `nexus.paneMode.activeViewId`.
  Manipulates the shared `usePaneModeStore`.
- **Depends on:** Shell `paneModeStore`; nothing backend.
- **Verdict:** Useful
- **Rationale:** "Pane mode" is the full-body takeover used by canvas,
  graph, and viewBuilder; without it those plugins still render in their
  leaves, just without the maximized affordance. Not on the basic
  browse/edit path.

### nexus.viewBuilder

- **Path:** `shell/src/plugins/nexus/viewBuilder/`
- **Surface:** Registers `viewBuilder` view in the left rail and an
  activity-bar entry; commands `nexus.viewBuilder.show`,
  `saveLayoutAs`, `switchLayout`. Reads / writes named layout JSON under
  `<forge>/.forge/layouts/`, exports a layout as a stub plugin.
- **Depends on:** `nexus.workspace` (`layoutSnapshot`, `applySnapshot`);
  backend storage IPC for `.forge/layouts/` files.
- **Verdict:** Optional
- **Rationale:** Power-user customisation surface. Layouts persist
  automatically via `workspace.json`; this only matters if the user wants
  multiple named layouts.

### nexus.canvas

- **Path:** `shell/src/plugins/nexus/canvas/`
- **Surface:** Registers the `canvas` view, claims the `.canvas` file
  extension via `viewRegistry.registerExtensions`; ~14 commands
  (`nexus.canvas.new`, undo/redo/delete, fit/fitSelection, toggleGrid,
  toggleBackground, tidy, export PNG/SVG/PDF, toggleHelp/closeHelp);
  configuration schema for export margins and color swatches.
- **Depends on:** `nexus.workspace`; backend `com.nexus.storage`
  (`canvas_read`, `canvas_write`, `canvas_*` handler family, `list_dir`).
- **Verdict:** Optional
- **Rationale:** Self-contained alternate editing surface for spatial
  diagrams. Absent canvas, `.canvas` files would fall through to a
  generic editor view. Orthogonal to markdown editing.

### nexus.notion

- **Path:** `shell/src/plugins/nexus/notion/`
- **Surface:** Two commands — `nexus.notion.import` and
  `nexus.notion.export`. Both prompt for paths through the Tauri dialog
  plugin and call `com.nexus.formats::import_notion` /
  `export_notion`. No view registration.
- **Depends on:** `@tauri-apps/plugin-dialog`; backend `com.nexus.formats`.
- **Verdict:** Optional
- **Rationale:** Migration / interop tooling. Not used in day-to-day
  editing; safe to omit from a minimal install. (Note: name is
  misleading — it lives under `notion/` but contributes nothing about
  block-style editing, only file import/export.)

## Category verdict

| Plugin               | Verdict   | Required for basic workflow |
|----------------------|-----------|-----------------------------|
| `nexus.editor`         | Essential | Yes — the real markdown editor |
| `nexus.multibufferSync`| Useful    | No — needed only for multibuffer features |
| `nexus.paneMode`       | Useful    | No — alternate views still render without it |
| `nexus.viewBuilder`    | Optional  | No                          |
| `nexus.canvas`         | Optional  | No — alternate file format  |
| `nexus.notion`         | Optional  | No — import/export only     |
