# Tab / note context-menu — development plan

Replicate Obsidian's per-note "⋯" menu (screenshot reference: the menu
that opens from the three-dot button in the note header) as a
contextual action palette attached to the active editor leaf.

Target entry point: the existing but disabled `more options` button at
`shell/src/plugins/nexus/editor/EditorView.tsx:398`.

## Goals

1. Match Obsidian's menu *structure* so users familiar with Obsidian
   don't relearn. Ordering, grouping, and separator placement all map
   1:1 in the first pass.
2. Ship the low-effort actions as working features; stub the rest as
   disabled menu rows with a consistent "Not yet implemented"
   affordance. The menu shape is load-bearing even when items are
   greyed — it signals intent and keeps the UI coherent while we
   backfill.
3. Route every action through the **command registry** (`api.commands.*`)
   so keyboard palettes and future keybindings compose for free.

## Scope split

### Phase 1 — easy wires (land together)

Each of these is a single-function command; most are already backed by
existing kernel/workspace APIs. Budget: ~30-45 min each.

| Action                      | Command id                       | Backed by                                       |
| --------------------------- | -------------------------------- | ----------------------------------------------- |
| Reading view / Source mode  | `nexus.editor.toggleMode`        | existing `mode` prop on EditorView              |
| Split right                 | `workspace.splitRight`           | `workspace.createLeaf` + split direction        |
| Split down                  | `workspace.splitDown`            | same, vertical                                  |
| Copy path (absolute)        | `file.copyAbsPath`               | `navigator.clipboard.writeText(rootPath + rel)` |
| Copy path (relative)        | `file.copyRelPath`               | `navigator.clipboard.writeText(rel)`            |
| Open in default app         | `file.openInDefaultApp`          | Tauri `@tauri-apps/plugin-shell` `open`         |
| Show in system explorer     | `file.revealInOS`                | Tauri shell `open` on parent dir                |
| Find...                     | `editor.find`                    | CodeMirror `openSearchPanel`                    |
| Replace...                  | `editor.replace`                 | CodeMirror `openSearchPanel` (replace mode)     |
| Reveal file in navigation   | `files.revealActive`             | files plugin store's `setSelected` + expand     |
| Delete file                 | `file.delete`                    | kernel `delete_file` + confirm dialog           |

Together with Phase 2, this gives users a visibly complete menu the
first time they right-click a tab.

### Phase 2 — stubs with tooltips

These render in the menu at their canonical position but are disabled,
with a tooltip explaining the state. Keeps menu shape right while the
real work is queued.

- Rename...
- Move file to...
- Bookmark...
- Merge entire file with...
- Add file property
- Export to PDF...
- Open in new window
- Open version history
- Open linked view
- Backlinks in document

Tooltip copy: `"Not yet implemented"` (single string, reused). Disabled
rows use the same muted styling as existing disabled buttons; no
separate "coming soon" badge — the tooltip is enough.

### Phase 3 — real features (follow-ups, one per ticket)

Ordered by user value, cheapest first:

1. **Rename / Move file to** — one kernel op (`rename_file`). Needs a
   small modal for the target path; reuse the `nexus.confirm` dialog
   shell. The workspace should update its open leaves on rename
   (`editorStore.updateRelpath`).
2. **Reveal backlinks in document pane** — already have a backlinks
   plugin; just need a focus command that reveals + activates its leaf
   in the right dock.
3. **Bookmark…** — wire to the bookmarks plugin's store. Prompt for
   title on add.
4. **Add file property** — small frontmatter inspector, probably a
   drawer inside the editor view. Non-trivial because it has to parse
   + serialize YAML and reconcile with the CRDT content.
5. **Export to PDF** — Tauri's webview has `webview.print()`; cheapest
   path is a print dialog. A full "export" with styled rendering is a
   follow-up to that.
6. **Open in new window** — Tauri multi-window. Needs a secondary
   window spec plus handoff of workspace/forge state.
7. **Open version history** — requires a new history plugin backed by
   either git history (if forge is a repo) or a lightweight file-level
   snapshot store. Large; out of scope for this menu.
8. **Merge entire file with...** — blocked on a merge-view design; not
   scheduling yet.
9. **Open linked view** — submenu of the other leaves currently
   displaying this file; lightweight once multi-window lands.

## Implementation notes

### Menu component

Build a shared `ContextMenu` primitive in
`shell/src/shell/ContextMenu.tsx` modelled on the existing
`TabListDropdown` in `workspace/WorkspaceRenderer.tsx`:

- Controlled `open` state, anchor ref, `position: fixed` popover,
  outside-click + Escape dismissal.
- Items declared as a flat array of `{ kind, commandId?, label,
  submenu?, separator?, disabled?, accelerator? }` records.
- Separators render as 1px `var(--divider-color)` rows.
- Submenus (`Copy path`, `Open linked view`) open on hover to the right,
  mirroring Obsidian's positioning.

### Wiring

- `EditorView.tsx` enables the `⋯` button when a tab is active. Clicking
  it opens the menu anchored to the button.
- Item records live next to the EditorView (or a sibling file
  `TabContextMenu.tsx`) because most actions need the active tab's
  `relpath`. The menu component itself should be generic.
- Actions that operate on the active leaf (split, find, replace, mode
  toggle) read the leaf from the workspace store; they don't take
  `relpath` arguments.

### Telemetry / regressions

- Wdio spec `e2e/specs/tab-context-menu.spec.ts` covers the menu
  opening, picking "Reading view", and "Copy path".
- No Rust-side changes in Phase 1 — everything is frontend on top of
  existing kernel commands.

## Out of scope

- Right-click context menu on tabs themselves (same menu, different
  anchor — trivial to add later once the shared primitive exists).
- Keyboard accelerators on menu items. The command registry already
  supports keybindings; adding menu hints is cosmetic and can follow.
- Theme-aware icon glyphs inside the menu. First pass uses the same
  Lucide icons already in the Ic registry.
