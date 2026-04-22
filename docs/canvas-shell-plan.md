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

Budget: 1 day.

- **Edge creation**: drag from a node's border handle (shown on
  hover) onto a target node.
- **Edge editing**: click to select; side inspector for label / color /
  line style.
- **Edge deletion**: `delete` while edge is selected.
- **Inspector panel**: floating drawer on the right when a node or
  edge is selected — properties editor (label, color, type, size).

#### Phase 5 — node body embeds

Budget: 1–2 days total, each node type incrementally.

- `file` node: embed the target file's rendered content inline,
  scrollable within the node's bounds. Reuse the editor's render
  pipeline so markdown extensions work.
- `database` node: inline mini-grid of the linked `.bases`. Reuse
  Phase-2 Bases Table view at reduced density.
- `terminal` node: read-only transcript + a "Run" button that hands
  the command to `com.nexus.terminal::create_session` +
  `send_raw_input`, streaming output back into the card via
  `read_raw_since` — same plumbing as the main terminal drawer.
- `text` node: full markdown rendering (not just raw); supports
  wikilinks and tags.
- `link` node: OG-metadata card (title + favicon + description) using
  an offline-friendly fallback when the link can't be fetched.

#### Phase 6 — polish

- **Minimap** in the corner showing full canvas with a viewport rect.
- **Auto-layout**: one-click "tidy" that runs a force-directed pass.
- **Export**: PNG / SVG / PDF of the current canvas (Tauri print pipe
  or html2canvas).
- **Keyboard shortcuts** (all of the above): documented + configurable.
- **Grid toggle** in the bottom-right control strip.
- **Background color / pattern** per-canvas as a file-level setting.

## Phasing recap

- Phase 1: routing + blank canvas leaf — no regression when opening
  `.canvas`.
- Phase 2: render + camera — can view existing Obsidian canvases.
- Phase 3: interactions — can create + rearrange.
- Phase 4: edges + inspector — fully editable graph.
- Phase 5: rich node embeds — feature parity with Obsidian.
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
