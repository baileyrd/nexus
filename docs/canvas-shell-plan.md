# Canvas (`.canvas` boards) — shell implementation plan

Bring `.canvas` file support to the current shell UI. The format spec
is locked (PRD-06 §4 — Obsidian-compatible v1.0 JSON), and the storage
layer can parse, serialize, insert, query, and delete canvas files
plus extract their file links. What's missing is IPC exposure of those
functions and an actual canvas editor component in the shell.

References:
- **Format spec**: `docs/PRDs/06-file-formats.md` §4 (JSON schema, node
  types, edge types, Obsidian compatibility).
- **Storage evidence**: `crates/nexus-storage/src/canvas.rs` —
  `parse_canvas`, `serialize_canvas`, `insert_canvas`, `query_canvas_nodes`,
  `query_canvas_edges`, `delete_canvas`, `extract_file_links`.
- **Backlog note**: `docs/PRDs/BACKLOG_COMPLETED.md` line 84 —
  opening a `.canvas` file currently falls through to CodeMirror and
  displays raw JSON. File-handler registration is live, just unused
  for `.canvas`.

## Goal

Open a `.canvas` file in the main dock and render it with a zoomable,
pannable, editable infinite canvas. Parity targets: file embed nodes,
text cards, external link cards, groups, `database` nodes (referencing
a `.bases` file), and `terminal` nodes. Drag to move, drag to create
edges, keyboard shortcuts for common ops.

## What's already wired (don't rebuild)

- **Parsing / serialization** of the Obsidian-compatible v1.0 JSON
  schema in `crates/nexus-storage/src/canvas.rs`.
- **SQLite indexing** of nodes + edges: `insert_canvas` /
  `query_canvas_nodes` / `query_canvas_edges` / `delete_canvas`.
- **File-link extraction** so graph view and link queries include
  canvas-originated edges (`extract_file_links`).
- **CLI**: `nexus canvas` subcommand group for all operations.
- **Frontend file-handler contract**: ready to route `.canvas` to a
  dedicated content type.

## What's missing (build this)

### Kernel gap: IPC handlers

`canvas.rs` exposes Rust functions but no `com.nexus.*` IPC handlers
reach them from the webview today. Add to `com.nexus.storage`:

| Handler            | Purpose                                                       |
| ------------------ | ------------------------------------------------------------- |
| `canvas_read`      | Load a `.canvas` JSON + return parsed document                |
| `canvas_write`     | Serialize + atomic write (reuse `atomic.rs`)                  |
| `canvas_patch`     | Apply a minimal diff (node add/move/delete + edge add/delete) |
| `canvas_nodes`     | Paged node query (reuse `query_canvas_nodes`)                 |
| `canvas_edges`     | Paged edge query (reuse `query_canvas_edges`)                 |

`canvas_patch` is the hot path — drag operations shouldn't rewrite the
whole file per frame. Debounce at the shell layer, flush on idle /
close / save.

Register the string names in the bootstrap plugin (same table as
`pump` / `read_output` / `read_raw_since`).

### Shell UI phases

> **Status (2026-04-22):** Phases 1–5 complete. Next pickup is
> Phase 6 (polish — minimap, auto-layout, export, grid toggle, etc.).
> Everything listed below is live in the shell:
>
> - Kernel surface: `canvas_read` / `canvas_write` / `canvas_patch` /
>   `canvas_nodes` / `canvas_edges` on `com.nexus.storage` (handler
>   ids 35–39).
> - New plugin `com.nexus.linkpreview` (`nexus-linkpreview` crate) —
>   one handler, `fetch`, for OG/Twitter/HTML-title metadata.
> - Full Phase-3 interaction model (select / marquee / drag-move /
>   resize / delete / create text / drag-from-edge-to-create / undo
>   / redo).
> - Phase-4 edges + inspector — bezier hit-testing, selected-edge
>   highlight, delete on edge, floating inspector for node + edge
>   properties (all edits patch through `*_update` ops).
> - Phase-5 DOM overlay layer (`CanvasOverlay.tsx`) mounting
>   camera-tracked, pointer-events-none HTML per node. Every non-
>   group node body lives there now: text → markdown, file →
>   markdown/image/text preview, link → OG card with favicon + hero,
>   database → mini-grid of `.bases` schema + records, terminal →
>   Run/Stop button + ANSI-stripped PTY transcript. The 2D canvas
>   draws card chrome only for non-group nodes.
>
> Shell code lives under `shell/src/plugins/nexus/canvas/`
> (`CanvasOverlay.tsx`, `Inspector.tsx` are the Phase-4/5 additions).

#### Phase 1 — view registration + blank surface

Budget: half a day.

- Create `shell/src/plugins/nexus/canvas/`:
  - `index.ts` — plugin registration, `.canvas` → `'canvas'` file
    handler, activity-bar entry optional.
  - `CanvasPaneView.tsx` — `ViewBase` subclass mounting a React root
    (follow `MarkdownView` / `TerminalPaneView`).
  - `CanvasView.tsx` — root component (empty surface in this PR).
  - `canvasStore.ts` — zustand store: parsed document, camera (x/y/
    zoom), selection, pending patches.
  - `kernelClient.ts` — wrappers around the new IPC handlers.
- Register `'canvas'` view type with the workspace's `viewRegistry`.
- Phase 1 ships when opening a `.canvas` file shows a panning grey
  canvas with a node count in the corner. Proves routing +
  `canvas_read` end-to-end.

#### Phase 2 — renderer + camera

Budget: 1–2 days.

- **Renderer**: canvas-based (not SVG). 500+ nodes should pan at
  60 fps. SVG DOM gets punishing past a couple hundred nodes.
- **Camera**: wheel to zoom (anchored on pointer), middle-click or
  space-drag to pan, pinch-zoom on trackpad. Clamp zoom to a sensible
  range (0.1× – 4×).
- **Node render paths** (one per node type):
  - `file` — embed preview of linked `.md`/`.mdx`/`.canvas`. First pass
    renders the basename + a thumbnail; full embed in Phase 5.
  - `text` — rounded card with rendered markdown body.
  - `link` — external-URL card with fetched OG metadata (best effort).
  - `group` — translucent rectangle with a label tab.
  - `database` — pill card showing `.bases` name + row count.
  - `terminal` — monospace command card; "Run" button is Phase 5.
- **Edges**: curved or orthogonal lines with an optional label. Solid
  / dashed / dotted per the PRD; arrow heads based on `from`→`to`.
- **Styling**: respect theme tokens; node colors fall back to
  `--bg-raised` when `color` isn't set; edge color defaults to
  `--fg-muted`.

#### Phase 3 — interactions

Budget: 2 days.

- **Selection**: click node to select, shift-click to add, drag empty
  space to marquee-select.
- **Move**: drag selected nodes. Snap-to-grid toggle (default off).
  Coalesce into a single `canvas_patch` on mouseup.
- **Resize**: handles on node corners/edges. Lock aspect with shift.
- **Create**:
  - Double-click empty space → text node with inline editor.
  - Drag from empty space → marquee = group node (post-release prompt
    for label).
  - Drag from a node's edge → creates a new node at drop site + edge
    connecting them (Obsidian-style).
- **Delete**: `delete` / `backspace` removes selected nodes + incident
  edges.
- **Undo / redo**: client-side stack scoped to the session. Each
  redo/undo produces a `canvas_patch`.
- **Zoom-to-fit** / **zoom-to-selection** keyboard shortcuts.

#### Phase 4 — edges + inspector

Budget: 1 day. **Done 2026-04-22.**

- **Edge creation**: drag from a node's border handle (shown on
  hover) onto a target node. Landed in Phase 3.
- **Edge editing**: click to select; floating inspector for label /
  color / line style. Landed.
- **Edge deletion**: `delete` while edge is selected. Landed.
- **Inspector panel**: floating drawer on the right when a node or
  edge is selected — properties editor (label, color, type, size).
  Landed. Multi-select node property editing is still out of scope;
  the drawer binds only when exactly one node is selected.

#### Phase 5 — node body embeds

Budget: 1–2 days total, each node type incrementally. **Done 2026-04-22.**

- `text` node — full markdown rendering via the shared
  `renderMarkdown` pipeline (marked + DOMPurify). Landed in 5a.
- `link` node — OG/Twitter/HTML-title card (favicon + site + title +
  description + hero image) via new `com.nexus.linkpreview::fetch`
  handler (`nexus-linkpreview` crate, reqwest + regex). Offline
  fallback = hostname + raw URL. Landed in 5b.
- `file` node — inline preview of the linked forge file: markdown,
  images (as base64 data URLs), text/code (as monospaced `<pre>`),
  or a "no preview" placeholder for binaries. Text capped at 64
  KiB with a truncated indicator. Landed in 5c.
- `database` node — mini-grid of the linked `.bases` schema +
  records (first 4 columns, first 50 rows) via
  `com.nexus.storage::base_load`. Landed in 5d.
- `terminal` node — Run / Stop button + live PTY transcript via
  `com.nexus.terminal::{create_session, send_input, read_raw_since,
  close_session}`. ANSI-stripped, last 32 KiB visible, bottom-
  anchored so newest output stays on screen. Session torn down on
  unmount / stop. Landed in 5e.

#### Phase 6 — polish  ← **pickup here**

- **Minimap** in the corner showing full canvas with a viewport rect.
- **Auto-layout**: one-click "tidy" that runs a force-directed pass.
- **Export**: PNG / SVG / PDF of the current canvas (Tauri print pipe
  or html2canvas).
- **Keyboard shortcuts** (all of the above): documented + configurable.
- **Grid toggle** in the bottom-right control strip.
- **Background color / pattern** per-canvas as a file-level setting.

## Phasing recap

- Phase 1: routing + blank canvas leaf — no regression when opening
  `.canvas`. **Done.**
- Phase 2: render + camera — can view existing Obsidian canvases.
  **Done.**
- Phase 3: interactions — can create + rearrange. **Done.**
- Phase 4: edges + inspector — fully editable graph. **Done.**
- Phase 5: rich node embeds — feature parity with Obsidian. **Done.**
- Phase 6: polish.

Phase 1–3 is the minimum to call `.canvas` a first-class surface.

## Implementation notes

### Performance

- Use `requestAnimationFrame` for renders; batch state changes.
- Spatial index (R-tree or grid hash) for hit-testing if node count
  grows large. Not needed < 500 nodes.
- Dirty-rect rendering once static regions dominate.

### Persistence

- Debounce `canvas_patch` calls — one flush per ~300 ms of idle, plus
  a flush on blur / close / save.
- Session undo stack never crosses a save boundary; `ctrl+z` after
  save prompts before rewinding.

### Obsidian compat

- Preserve unknown fields when reading + writing so a canvas edited in
  Obsidian doesn't lose data in Nexus.
- Our `nexus-storage::canvas::CanvasFile` should serde with
  `#[serde(deny_unknown_fields = false)]` plus an `extra:
  serde_json::Map` catch-all. If not already, add in the IPC-handler
  PR.

## Out of scope

- Real-time collaborative editing.
- Canvas-to-document export (turn a canvas into a linear outline).
  Interesting feature, but a separate plan.
- Canvas templates gallery.
- Public-facing shareable canvases.
- Animated node transitions beyond the default move-and-resize.
