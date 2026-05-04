# Obsidian-parity tracker — Canvas + Bases + Graph + Activity Bar

Snapshot date: 2026-05-04. Maps every visible affordance in the
Obsidian Canvas, Bases, and Graph views to its Nexus state — done,
partial, or missing — and lists shippable bites in priority order.

Source files in Nexus:
- Canvas: `shell/src/plugins/nexus/canvas/`
- Bases:  `shell/src/plugins/nexus/bases/`
- Graph:  `shell/src/plugins/nexus/graph/`
- Activity bar: `shell/src/plugins/core/activityBar/`

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

## 3. Graph (global graph view)

### 3.1 Surface and interaction

| Affordance | Nexus state | Notes |
|---|---|---|
| Force-directed layout (nodes + edges from link graph) | ✅ | `GraphGlobalView.tsx` + `forceLayout.ts`. |
| Pan / zoom | ✅ | |
| Click node → open file | ✅ | |
| Per-file local graph (just neighbours of the active file) | ✅ | `GraphView.tsx`. |
| Settings drawer (gear) | ✅ | `GraphGlobalGearDrawer.tsx`. |

### 3.2 Filters section

| Filter | Obsidian | Nexus | Notes |
|---|---|---|---|
| Search files… | ✅ | 🟡 | Nexus's "Path filter" accepts substring/glob; Obsidian search matches names. Same intent, different UX. |
| Tags toggle | ✅ | ⚪ | Show/hide tag nodes in the graph. Not modelled in Nexus's graph today. |
| Attachments toggle | ✅ | ⚪ | Show/hide non-markdown files. Missing. |
| Existing files only | ✅ | 🟡 | Nexus has the inverse: "Include unresolved links". Same control, opposite default. |
| Orphans (no in/out links) | ✅ | ✅ | Nexus's "Include orphan nodes" maps directly. |

### 3.3 Groups section

Obsidian lets the user define named groups (with colour) by glob/path
matcher; nodes get coloured by group membership.

| Element | Nexus state | Notes |
|---|---|---|
| New group button | ⚪ | Nexus has only `Colour by folder` (a single boolean), not user-defined groups. |
| Coloured nodes by group | ⚪ | Tied to (1). |
| Per-group remove | ⚪ | |

### 3.4 Display section

| Control | Obsidian | Nexus | Notes |
|---|---|---|---|
| Arrows toggle | ✅ | ⚪ | Render edges as directed arrows. Nexus draws plain lines. |
| Text fade threshold slider | ✅ | ⚪ | Hide labels below a zoom threshold. |
| Node size slider | ✅ | ⚪ | Adjust node radius. |
| Link thickness slider | ✅ | ⚪ | |
| Animate button | ✅ | ⚪ | One-shot re-layout animation. |
| Show labels toggle | – | ✅ | Nexus-only — Obsidian always shows labels. |
| Freeze simulation | – | ✅ | Nexus-only convenience. |

### 3.5 Forces section

| Force | Obsidian | Nexus | Notes |
|---|---|---|---|
| Center force | ✅ | ✅ | Nexus's "Center gravity". |
| Repel force | ✅ | ✅ | Nexus's "Repulsion". |
| Link force | ✅ | ✅ | Nexus's "Link strength". |
| Link distance | ✅ | ✅ | Same name. |

### 3.6 Right-rail toolbar

| Button | Obsidian | Nexus | Notes |
|---|---|---|---|
| ⚙ Settings (filters/groups/display/forces) | ✅ | ✅ | Wires to gear drawer. |
| 🎨 Start timelapse animation | ✅ | ⚪ | Walks the graph in chronological order (file ctime), animating nodes appearing as their files were created. |

### 3.7 Tab-actions menu (`⋮`)

| Item | Nexus state | Notes |
|---|---|---|
| Split right / Split down | ✅ | Existing tab-actions menu. |
| Copy screenshot | ⚪ | Copy the rendered graph to the clipboard as PNG. |
| Bookmark | ✅ | Existing. |

### 3.8 Outstanding bites for Graph (priority order)

1. ⚪ **Display section parity** — Arrows toggle, Text fade threshold,
   Node size, Link thickness, Animate button. ~120 lines (slider
   primitives + force-layout config plumbing).
2. ⚪ **Tags + Attachments filters** — extend the graph data model so
   tags become first-class nodes and attachments are filterable. ~150
   lines.
3. ⚪ **Named groups** — replace `colourByFolder` with a list of
   user-defined groups (glob → colour); add UI for create/edit/delete.
   ~200 lines + new persisted settings shape.
4. ⚪ **Existing files only** — add the inverse-of-unresolved toggle
   to match Obsidian's default. Trivial. ~10 lines.
5. ⚪ **Copy screenshot** — render to canvas, write blob to clipboard
   via `navigator.clipboard.write`. ~50 lines.
6. ⚪ **Timelapse animation** — order nodes by ctime, animate their
   appearance over a configurable duration. ~150 lines + a small
   timeline overlay.

---

## 4. Activity bar (left rail)

The Obsidian left rail shows a vertical stack of action buttons; each
hover reveals a tooltip naming the action. The Nexus equivalent is the
`core.activity-bar` plugin with seeded entries (Search, Graph, Tasks,
Git, Bases, Templates, AI) plus per-plugin `api.activityBar.addItem`
contributions from feature plugins.

| Tooltip seen in Obsidian | Nexus equivalent | State |
|---|---|---|
| Open quick switcher | core seed `rail.search` (Search) | 🟡 Quick switcher itself isn't a separate plugin in Nexus; the Search rail item maps loosely. |
| Open graph view | `nexus.graph` (`globalIndex.ts` registers an activity-bar entry) | ✅ |
| Create new canvas | `nexus.canvas.new` (committed in d409a77 / 20b998f) | ✅ |
| Open today's daily note | – | ⚪ No daily-notes plugin exists yet. |
| Insert template | `nexus.templates` | ✅ |
| Open command palette | `nexus.commandPalette` (core seed) | ✅ |
| Create new base | `nexus.bases.new` | ✅ |
| Start/stop recording | – | ⚪ No audio-recorder plugin exists yet. |

### 4.1 Outstanding bites

1. ⚪ **Daily Notes plugin** — register an activity-bar entry that
   resolves the date format → relpath, opens (or creates) today's
   note. Settings stub already exists at `cp-stub:daily-notes`.
2. ⚪ **Audio Recorder plugin** — `MediaRecorder` API → write WebM/Opus
   under the configured attachments folder, drop a markdown link at
   caret. Activity-bar mic button toggles record state.
3. 🟡 **Quick switcher** vs Search — Obsidian's quick switcher is a
   focused file picker (Ctrl/Cmd+O), separate from full-text search.
   Today the rail item routes to Search; consider promoting the file-
   open command palette to its own rail action.

---

## 5. Cross-feature notes

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

## 6. References

- ADR 0019 — Obsidian base format (`.base` is read-only): `docs/adr/0019-obsidian-base-format.md`
- Bases shell plan: `docs/archive/bases-shell-plan.md`
- Canvas shell plan: `docs/archive/canvas-shell-plan.md`
- Settings stubs audit: `docs/research/settings-stubs-audit.md`
- Obsidian-vs-Nexus API divergence: `docs/research/obsidian-vs-nexus-api.md`
