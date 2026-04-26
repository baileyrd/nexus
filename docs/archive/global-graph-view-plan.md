> **Archived 2026-04-26** — Implementation plan for the global graph view. Shipped — see `shell/src/plugins/nexus/graph/GraphGlobalView.tsx`.

# Global graph view — development plan

Replace the current local-neighborhood graph with an Obsidian-style
global graph that renders every note in the forge as a node and every
wiki-/markdown-link as an edge. Opens as a main-dock tab, not a
right-panel sidecar.

Reference: the Obsidian "Graph view" tab — force-directed layout,
floating gear menu for settings in the top-right, zoom/pan, node
hover/highlight, and a separate reset-zoom affordance bottom-right.

## Current state

- `src/plugins/nexus/graph/` — graph plugin exists but:
  - Scoped to the active file's neighborhood (1-hop links only).
  - Registers as a right-panel tab via `rightPanel:registerTab`.
  - Pulls data from `com.nexus.storage::outgoing_links` + `backlinks`,
    one call per active file.
- `crates/nexus-storage/src/graph.rs` — has the full link graph
  in-memory (built during vault scan) but only exposes per-file queries
  over IPC. No bulk "give me every edge" handler today.

## Target state

Main-dock tab (view type `graph`) showing every note and every resolved
link across the current forge. Opens from an activity-bar button or a
command (`nexus.graph.openGlobal`) using
`workspace.ensureLeafOfType('graph', 'main')` — same shape we use for
workflow / mcp / skills / ai-chat so it shares dedupe + reveal
semantics.

## Scope split

### Phase 1 — backend: bulk graph handler

Add a single IPC handler on `com.nexus.storage` that returns the forge's
full link graph in one call. This is the load-bearing piece; without
it, the frontend would have to make N separate calls (one per note),
which doesn't scale past a few hundred files.

New handler: `list_all_links`.
Response shape: `{ nodes: { path: string, title: string? }[], edges: { source: string, target: string, is_resolved: bool }[] }`.

- Register in the string→id table at
  `crates/nexus-bootstrap/src/lib.rs` (same spot where
  `pump`/`read_output`/`read_raw_since` live). Reuse an unused handler
  id.
- Implementation reads the in-memory graph index that already exists in
  `nexus-storage`. No DB round-trip; just a projection of existing
  `Graph` state into the response shape.
- Unit tests in `nexus-storage` for: empty forge, one note no links,
  one note with unresolved link, cycle of two notes, dense subgraph.

### Phase 2 — frontend data + placement

Re-shape `src/plugins/nexus/graph`:

- Add a new `graphGlobalStore` tracking `{ nodes, edges, loadedAt,
  loading, error }`. Keep the existing neighborhood store intact for
  now; we'll retire it in Phase 4 once global lands.
- New view type `'graph'` (not `'nexus.graph.view'`) registered
  against the workspace viewRegistry. The neighborhood pane keeps its
  right-dock registration on a different view type until we retire it.
- Command `nexus.graph.openGlobal` →
  `workspace.ensureLeafOfType('graph', 'main')` + `revealLeaf`. Wire an
  activity-bar item with the graph icon that fires this command.
- Load on leaf `onOpen`; refresh on `workspace:opened` and on file
  changes coming from the storage plugin's event topics (needs a
  debounce to coalesce bulk edits).

### Phase 3 — rendering

Force-directed layout with zoom / pan. D3-force or a lightweight
stand-in is fine; keep to pure-frontend (no WebGL dependency in the
first pass).

- Canvas-based render for node counts > a few hundred; fall back to
  SVG if canvas proves hard to theme. Canvas is the right call even at
  500 nodes with lines.
- Interactions: wheel to zoom, drag background to pan, drag node to
  move, click node to open the file (dispatches
  `editor:openFile` event with the `relpath`), hover to highlight
  neighbours.
- Colours: default muted grey, hover/active accent, unresolved links
  dimmed. Respect `--accent` and the forge theme tokens.
- Floating overlay in the top-right corner: gear icon (settings
  drawer — placeholder in Phase 3), reset-zoom button. Bottom-right:
  zoom level label. Mirror Obsidian's layout.

### Phase 4 — settings + polish

Gear drawer content (each is a small commit, not a prerequisite for
Phase 3 shipping):

- **Filters**: include/exclude by path glob, include unresolved links,
  include orphan nodes.
- **Groups**: colour-code nodes by folder or tag. Simple palette, four
  slots to start.
- **Display**: toggle labels, tweak repulsion / link distance, freeze
  simulation.
- **Forces**: center gravity, link length, link strength sliders.

Persist settings per-forge in a small `graph.json` under `.forge/`.

## Implementation notes

### Performance

- Build the force simulation off the main thread (Web Worker) once
  node count exceeds ~300. Before that, a synchronous tick-per-frame
  is fine.
- Cache the node positions between ticks so mouse-drag doesn't reset
  the layout. Freeze on user interaction, resume on release.

### Event wiring

- Reuse the storage plugin's existing `com.nexus.storage.linkGraph.*`
  event topics (already broadcast on scan completion). If those topics
  don't exist yet, add them in the same PR as the bulk handler — the
  graph view needs them to refresh on vault edits.
- Debounce refreshes to one every ~500 ms; a burst of saves in a large
  forge would otherwise thrash the force simulation.

### File-action integration

- Clicking a node fires `editor:openFile` with `{ relpath }`, same
  event `nexus.files` / the quick-switcher already use. No new path.
- Optional: respect the existing `nexus.files.revealActive` when the
  global graph is visible — selecting a node in the graph scrolls the
  file tree to it.

## Out of scope

- Local-neighborhood graph retirement. Keep both plugins alive until
  the global graph is proven; the neighborhood view is genuinely useful
  as a sidecar. Once global ships we can decide whether to delete the
  old pane or keep it as a `graph-local` type.
- Canvas-style freeform drawings (Obsidian's other "Canvas" feature).
  That's an entirely separate tool.
- Multiple graph tabs per forge. One leaf of view type `graph` at a
  time is the expected behavior; `ensureLeafOfType` enforces this.
- Shared cursors / multi-user. Single-user view only.

## Rough sequencing

Land in this order, one PR per phase:

1. `nexus-storage::list_all_links` handler + bootstrap registration +
   tests.
2. Frontend plumbing: store, view registration, command, activity-bar
   item. View can render a "loading…" placeholder in this PR.
3. Canvas force-directed renderer with zoom/pan/drag + click-to-open.
4. Gear drawer + filter/group/display settings. Each sub-feature can
   be its own commit within Phase 4.

Phases 1–3 together give a usable product; Phase 4 is the long tail.
