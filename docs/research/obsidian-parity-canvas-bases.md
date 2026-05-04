# Obsidian-parity tracker — Canvas + Bases

Snapshot date: 2026-05-04. Maps every visible affordance in the
Obsidian Canvas and Bases views to its Nexus state — done, partial, or
missing — and lists shippable bites in priority order.

Source files in Nexus:
- Canvas: `shell/src/plugins/nexus/canvas/`
- Bases:  `shell/src/plugins/nexus/bases/`

Status legend: ✅ done · 🟡 functional but different shape · ⚪ missing.

---

## 1. Canvas

### 1.1 Surface and interaction

| Affordance | Nexus state | Notes |
|---|---|---|
| 2D canvas + DOM overlay (text / link / file / database / terminal nodes) | ✅ | `CanvasView.tsx` + `CanvasOverlay.tsx`. |
| Pan (middle-button + wheel) | ✅ | |
| Zoom (Ctrl/Cmd+wheel) | ✅ | |
| Marquee select | ✅ | |
| Shift+click multi-select | ✅ | |
| Multi-node drag | ✅ | |
| Double-click empty space → new text node | ✅ | |
| Drag from node border → create connected node | ✅ | |
| Edge resize / reroute | ✅ | |
| Block-ref drop (heading drag from another doc → file-ref node) | ✅ | `blockRefDrop.ts`. |
| Toggle grid, toggle background, tidy / auto-layout | ✅ | |
| Undo / redo with full inverse-op history | ✅ | |
| Help overlay (`?`) | ✅ | |
| Export PNG / SVG / PDF | ✅ | |

### 1.2 Right-rail toolbar (Obsidian-style)

| Button | Nexus state | Notes |
|---|---|---|
| ⚙ Settings popup | ✅ | `CanvasRightRail.tsx`. Snap to grid is real (wired to existing toggleGrid); Snap to objects + Read-only are stubs. |
| ⊕ Zoom in | ✅ | `handle.zoomBy(1.2)` around viewport centre. |
| ↻ Reset zoom | ✅ | `handle.resetZoom()`. |
| ⛶ Zoom to fit | ✅ | Existing `runFit(false)`. |
| ⊖ Zoom out | ✅ | `handle.zoomBy(1/1.2)`. |
| ↶ Undo / ↷ Redo | ✅ | Wired to existing handle methods. |
| ❓ Canvas help | ✅ | Existing help overlay. |

### 1.3 Bottom drag rail

| Source | Nexus state | Notes |
|---|---|---|
| Drag to add card (blank text node) | ✅ | `application/x-nexus-canvas-card` MIME → drop creates empty text node at cursor. |
| Drag to add note from vault | 🟡 | Drag handle exists, fires "Coming soon" toast on dragend. Needs vault-file picker / drop integration. |
| Drag to add media from vault | 🟡 | Same shape as note. |

### 1.4 Settings popup options

| Option | Nexus state | Notes |
|---|---|---|
| Snap to grid | ✅ | Real toggle. |
| Snap to objects | ⚪ | Stub. No backing logic. |
| Read-only | ⚪ | Stub. No read-only mode in canvas yet. |

### 1.5 Tab-actions menu (`⋮`)

The canvas tab inherits the editor's tab-actions menu, which already
contains Split right/down, Open in new window, Rename, Move, Bookmark,
Export as image, Copy path, Open version history, Open linked view,
Open in default app, Show in system explorer, Reveal file in
navigation, Delete file. Most of those are stubs — see
[settings-stubs-audit.md](settings-stubs-audit.md) for status.

### 1.6 Outstanding bites for canvas

1. ⚪ **Wire "Drag to add note from vault"** to the file picker so it inserts a file-ref node.
2. ⚪ **Wire "Drag to add media from vault"** to the same picker filtered to images / video.
3. ⚪ **Snap to objects** — needs alignment-detection during drag.
4. ⚪ **Read-only mode** — disable mutating ops, gate via toolbar toggle.
5. ⚪ **Image-paste / drag-drop from external** — paste a clipboard image directly into the canvas.

---

## 2. Bases

### 2.1 Layouts

| Layout | Obsidian | Nexus | Notes |
|---|---|---|---|
| Table | ✅ | ✅ | `BasesTable.tsx`. |
| Cards | ✅ | 🟡 | Nexus's `BasesGallery.tsx` is similar but isn't a 1:1 of Obsidian's Cards layout. |
| List | ✅ | ✅ | `BasesList.tsx`. |
| Board (kanban) | – | ✅ | Nexus-only. |
| Calendar | – | ✅ | Nexus-only. |
| Timeline | – | ✅ | Nexus-only. |

### 2.2 Toolbar — Obsidian shape

Obsidian shows a single thin row: `[ViewName ⌄ N results] ... Sort | Filter | Properties | Search | New`.

| Element | Nexus state | Notes |
|---|---|---|
| `[ViewName ⌄ N results]` left dropdown | 🟡 | Nexus today shows `name · N records · K fields · M views` plus a pill bar below. Functionally equivalent (named-view switcher, count display) but laid out as two rows, not one. |
| Sort button → popover | 🟡 | `setSort` + `setBoardGroupField` exist; UI is inline, not in a popover. |
| Filter button → popover (nested all/any tree) | 🟡 | `setViewFilters` + filter parsing exist; UI is more raw than Obsidian's nested-tree picker. |
| Properties button → popover (visibility list, Add formula, Hide all) | 🟡 | `setHiddenFields` exists. **Add formula is missing.** |
| Search input | ⚪ | Not present in Nexus. |
| New button | 🟡 | `nexus.bases.new` creates a *.base file*; Obsidian's "+ New" adds a *row* to the current base. Different scope. |

### 2.3 Configure view popover

Obsidian's Configure view popover combines: rename (View name input),
Layout (Table / Cards / List), Row height (Short / Medium / Tall /
Extra tall).

| Element | Nexus state | Notes |
|---|---|---|
| Rename | 🟡 | `BasesViewBar` has a per-view `⋯` menu with Rename/Duplicate/Delete. |
| Layout switcher | 🟡 | View-mode buttons in the header (table / board / list / calendar / gallery / timeline). |
| Row height | ⚪ | Not present. |

### 2.4 Sort popover

Obsidian: Group by [Property] [A→Z] [trash]; Sort by [Property] [A→Z] [trash]; + Add sort.

- Sort by single property: ✅ via `setSort`.
- Multi-key sort (multiple Sort by rows): ⚪ missing.
- Group by: 🟡 present for board (`setBoardGroupField`), not generalised across layouts.

### 2.5 Filter popover

Obsidian: nested `All views` / `This view` scopes, with `where ... links to ...` rules, and `All the following are true` / `Any of the following are true` group nodes; "+ Add filter" with a rich field picker (file, file name, file base name, file full name, file path, folder, file extension, created time, modified time, …).

- Filter rules: ✅ stored as rules in `BaseView.filter`.
- Nested all/any groups: 🟡 supported in the model, less polished in the UI.
- Field picker with file metadata fields (path, ext, ctime, mtime, …): 🟡 partial.
- All-views vs this-view scope: ⚪ missing — Nexus filters are per-view only.

### 2.6 Properties popover

Obsidian: alphabetised list of every available column with a checkbox per row, plus "Add formula" and "Hide all" buttons at the bottom.

- Column visibility checkbox list: 🟡 functional via `setHiddenFields`, less Obsidian-shaped.
- Add formula: ⚪ missing — no derived-column / formula support yet.
- Hide all: ⚪ missing — convenience action.

### 2.7 Search

| Element | Nexus state | Notes |
|---|---|---|
| Search field at top of view | ⚪ | New feature — text-match across visible records. |

### 2.8 New button (add row)

Obsidian's `+ New` button creates a new file (row) in the current
base's source folder and pops a quick-rename popover ("Untitled 2"
input + expand-to-full-editor button).

| Element | Nexus state | Notes |
|---|---|---|
| Add new row to current base | ⚪ | `nexus.bases.new` creates a new *.base file* (the schema), not a row inside an existing base. Need a new flow that: (a) creates a markdown file in the base's source folder, (b) seeds frontmatter required by the schema, (c) opens a quick-rename popover. |
| Quick-rename popover after creation | ⚪ | Tied to (a). |
| Expand-to-full-editor button | ⚪ | Switches from the popover to a regular editor tab. |

### 2.9 Outstanding bites for Bases (priority order)

1. ⚪ **Top toolbar reshape** — replace header + pill bar with a single
   `[ViewName ⌄ N results] ... Sort | Filter | Properties | Search | New`
   row. Wires existing functionality into popovers. ~250 lines. **Biggest visual parity win.**
2. ⚪ **Search bar** — quick text filter across visible records. ~80 lines.
3. ⚪ **Configure view popover** — combines rename + layout switch +
   row-height stub. Reshape of existing affordances. ~120 lines.
4. ⚪ **Sort popover with multi-key support** — promote single-key sort to
   an Obsidian-style ordered list. ~150 lines + store work.
5. ⚪ **Filter popover** — nested all/any tree with rich field picker.
   ~250 lines + filter-tree UI components.
6. ⚪ **Properties popover** — column visibility list + Hide all + Add
   formula stub. ~120 lines.
7. ⚪ **Add row + quick rename** — new IPC + popover. ~150 lines + Rust handler.
8. ⚪ **Cards layout** — adjust `BasesGallery` to match Obsidian's Cards
   shape (small previews, configurable cover field). ~100 lines.
9. ⚪ **Add formula columns** — derived columns; needs schema model
   change + evaluator. **Major.**

---

## 3. Cross-feature notes

- **Both areas have a "more actions" `⋮` button at the top-right** that
  routes through the editor tab-actions menu. Stubs for that menu are
  tracked separately in `docs/research/settings-stubs-audit.md`.
- **Read-only mode** keeps showing up as a Nexus gap (canvas right rail,
  Bases mode for `.base` files imported from Obsidian). A unified
  "leaf is read-only" concept might be worth a generalised
  implementation.
- **Vault file picker** is also missing for both surfaces (canvas drag
  rail's note/media sources, Bases formula reference picker). Consider
  building it once in a shared location.

## 4. References

- ADR 0019 — Obsidian base format (`.base` is read-only): `docs/adr/0019-obsidian-base-format.md`
- Bases shell plan: `docs/archive/bases-shell-plan.md`
- Canvas shell plan: `docs/archive/canvas-shell-plan.md`
- Settings stubs audit: `docs/research/settings-stubs-audit.md`
- Obsidian-vs-Nexus API divergence: `docs/research/obsidian-vs-nexus-api.md`
