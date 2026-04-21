# Leaf + ViewRegistry Migration Plan

Port Obsidian's `WorkspaceLeaf` / `ViewRegistry` / `setViewState` semantics into the Nexus shell. Replace the current `SlotRegistry`-based pane model for movable panes; keep `SlotRegistry` for fixed chrome only.

**Branch**: start from `no-titlebar-test` (current shell trunk).
**Reference**: `/home/baileyrd/projects/obsidian_reverse/docs/10-editor-shell.md` §§1–4. Cite that doc when implementing — it's the source of truth for Obsidian's behavior.

## Why

Obsidian's flexibility (drag any pane anywhere, popouts, layout persistence, plugin-contributed views) comes from three invariants:

1. **One primitive** (`WorkspaceLeaf`) for every movable pane — sidedock, center, floating. Same class, different parent.
2. **Views are registered by type key**, not hardcoded into slots. `ViewRegistry.register("graph", creator)`.
3. **`setViewState` is the only mutation path.** Serialization, history, drag-drop, popouts all reduce to it.

Current Nexus shell (`shell/src/registry/SlotRegistry.ts`) hardcodes `sidebar`, `editorArea`, `rightPanel`, `panelArea` as distinct slot IDs and registers React components directly. This prevents drag-between-regions, unified persistence, and a clean plugin view surface. The migration below fixes that without throwing away the existing plugin code — each current sidebar panel becomes a `View` subclass whose `onOpen(el)` mounts the existing React subtree.

## Non-goals

- Drag-and-drop between regions. Land the primitives first; drag is a follow-up.
- Floating/popout windows. The `FloatingWindow` node goes in the type model, but no Tauri multi-window work yet.
- Deleting `SlotRegistry`. Chrome positions (title bar, activity bar, status bar) stay as slots — they are genuinely fixed, not panes.
- Touching the Rust kernel. All changes are in `shell/src/`.

## Success criteria

- `shell/src/workspace/` module exists with `types.ts`, `ViewRegistry.ts`, `Leaf.ts`, `workspaceStore.ts`, `WorkspaceRenderer.tsx`.
- Current left/right sidebar contents (file-explorer, search, outline, backlinks, etc.) and the center editor render via `Leaf` + `View`, not via `SlotRegistry` slots `sidebar`/`sidebarContent`/`editorArea`/`rightPanel`/`rightPanelContent`/`panelArea`/`panelAreaContent`.
- `workspace.json` in the vault persists the layout; reloading the app restores every leaf's view and state by calling `setViewState` during hydration.
- Chrome slots (`titleBar`, `activityBar`, `statusBarLeft`, `statusBarRight`, `overlay`, `paneMode`) still work via `SlotRegistry` — unchanged.
- All existing plugin UIs continue to function. No regression in file tree, editor, outline, search, backlinks, graph, terminal, AI chat, MCP, settings.
- `tsc` clean, existing tests pass, manual smoke: create/open/close tabs, switch sidebar panels, collapse docks, reload app preserves layout.

## Phases

Each phase is a standalone commit. Do not collapse phases. The whole migration is ~1 week of focused work; phases are sized for ~½–1 day each.

### Phase 0 — Scaffolding (no behavior change)

Create `shell/src/workspace/` with the type surface. Nothing wires up yet.

**Files to create:**

- `shell/src/workspace/types.ts` — `View`, `ViewCreator`, `Leaf`, `ViewState`, `WorkspaceParent`, `Split`, `Tabs`, `Sidedock`, `Root`, `FloatingWindow`, `WorkspaceJSON` type-level shapes. Match the sketch in the design session (in memory; ask the user if missing). Key shape:
  - `Sidedock extends Split` (adds `side: 'left'|'right'`, `collapsed`, `size`). Not a separate discriminant — it's a `Split` with side metadata. Mirrors Obsidian's `FD extends OD`.
  - `Leaf.parent: WorkspaceParent` — placement is *where the leaf lives in the tree*, not a property of the leaf class.
  - `ViewState = { type, state?, active?, pinned?, group? }` — matches Obsidian exactly.
- `shell/src/workspace/index.ts` — barrel exports.

**Exit criteria:** `tsc` clean. No imports from other code yet.

### Phase 1 — ViewRegistry

**Files:**

- `shell/src/workspace/ViewRegistry.ts` — Zustand store with `creators: Map<string, ViewCreator>`, `extensions: Map<string, string>`, `register(type, creator): () => void`, `registerExtensions(exts[], type)`, `getCreator(type)`, `getTypeForExt(ext)`. Non-reactive read helper `viewRegistry` for use outside React, mirroring the pattern in `SlotRegistry.ts:76-82`.
- Built-in `empty` view creator registered at module load — the fallback when a leaf's view type is unknown or a file is closed. Obsidian does the same (`docs/10-editor-shell.md:178-182`); `{type:"empty"}` must be a legal persisted state.

**Exit criteria:** Unit test registers a creator, looks it up, unregisters, confirms gone. `empty` always resolvable.

### Phase 2 — Leaf implementation + `setViewState` choke-point

**Files:**

- `shell/src/workspace/Leaf.ts` — `class LeafImpl implements Leaf`. Implements `setViewState`, `getViewState`, `detach`. The `setViewState` algorithm follows `docs/10-editor-shell.md §3` literally:
  1. If current view exists: `await view.onClose()`, clear container element.
  2. Look up creator via `viewRegistry.getCreator(state.type)`. Fall back to `empty` if not found — do **not** throw. Matches Obsidian's resilience to missing plugin views on layout restore.
  3. Instantiate: `this.view = creator(this)`.
  4. If `state.state !== undefined`: `await view.setState(state.state, eState)`.
  5. If `containerEl` is mounted: `await view.onOpen(containerEl)`.
  6. Apply `pinned`, `group` from state.
  7. Emit `view-changed`; if `state.active`, emit `active-leaf-change` via `workspaceStore.setActiveLeaf`.
- `shell/src/workspace/View.ts` — abstract base class `ViewBase implements View` with default `getState()/setState()/onOpen()/onClose()` noops. Convenience for plugin authors.

**Implementation notes:**

- `containerEl` is assigned by `LeafHost` in React (Phase 4). Until then, `onOpen` calls are deferred — store the state and call on mount. This matches Obsidian's behavior when a leaf exists in serialized layout but its tab isn't yet visible.
- `setViewState` is `async` and must be awaitable. Callers in plugin code will `await leaf.setViewState(...)`.
- `view-changed` / `active-leaf-change` events go through `workspaceStore.emit` (Phase 3), not a separate emitter — single event bus for the workspace.

**Exit criteria:** Unit test creates a Leaf, calls `setViewState` with a registered type, asserts `view` is set and `onOpen` was called once with the container. Second `setViewState` call triggers `onClose` on the first view. Unknown type falls back to `empty`.

### Phase 3 — Workspace store

**Files:**

- `shell/src/workspace/workspaceStore.ts` — Zustand store with the layout tree:
  ```ts
  {
    rootSplit: Split
    leftSplit: Sidedock
    rightSplit: Sidedock
    floating: FloatingWindow[]
    activeLeafId: string | null
    leaves: Map<string, Leaf>
  }
  ```
  plus methods:
  - `createLeaf(parent: WorkspaceParent): Leaf` — instantiates `LeafImpl`, adds to `leaves` map and parent's children.
  - `getLeftLeaf(reveal?: boolean): Leaf` — returns first leaf in `leftSplit`, creates if empty. Matches `Workspace.prototype.getLeftLeaf` at `app.js` offset ~2,727,895 (`docs/10-editor-shell.md:129`).
  - `getRightLeaf(reveal?: boolean): Leaf` — mirror.
  - `ensureLeafOfType(type: string, side: 'left'|'right'): Promise<Leaf>` — if any leaf of that type exists anywhere in the workspace, return it unchanged (do not move it). If none exists, create a new leaf in the named sidedock and `setViewState({type})` it. Pure existence guarantee; does not touch visibility, dock-collapsed state, or active tab. See resolved decision #2 below — this deliberately diverges from Obsidian's `ensureSideLeaf`, which also moves + reveals.
  - `revealLeaf(leaf: Leaf): void` — expand the containing sidedock if collapsed, activate the tab within its tab group, and call `setActiveLeaf(leaf)`. Pure visibility. Compose with `ensureLeafOfType` when the intent is "make this pane visible to the user":
    ```ts
    const leaf = await workspace.ensureLeafOfType('file-explorer', 'left')
    workspace.revealLeaf(leaf)
    ```
  - `getLeavesOfType(type: string): Leaf[]` — flat scan of `leaves` map filtered by `view.viewType`.
  - `setActiveLeaf(leaf: Leaf): void` — updates `activeLeafId`, emits `active-leaf-change`.
  - `emit(event, payload)` — internal event bus. Events: `layout-change`, `layout-ready`, `active-leaf-change`, `view-changed`, `view-registered`, `pinned-change`. (Subset of Obsidian's; add more as plugins need them — `docs/07-plugin-api.md §4.1` has the full list.)
  - `serialize(): WorkspaceJSON` — walks tree, calls `leaf.getViewState()` on every leaf, returns JSON.
  - `hydrate(json: WorkspaceJSON): Promise<void>` — rebuilds tree, for each leaf `await leaf.setViewState(savedState)`. Emits `layout-ready` at end.

**Default layout** (when `workspace.json` is absent or corrupt):
- `rootSplit`: one `Tabs` group containing one `empty` leaf.
- `leftSplit`: one `Tabs` with a `file-explorer` leaf.
- `rightSplit`: one `Tabs` with an `outline` leaf.

Match the visual layout currently produced by the shell so there's no UX regression on first run.

**Exit criteria:** Hydrate from a fixture `WorkspaceJSON`, assert the tree shape and every leaf's `view.viewType` matches. Serialize round-trips (deep-equal) through hydrate → serialize.

### Phase 4 — React render layer

**Files:**

- `shell/src/workspace/WorkspaceRenderer.tsx` — `<Workspace>`, `<RenderNode>`, `<TabGroup>`, `<TabStrip>`, `<LeafHost>`, `<SidedockFrame>`.
  - `<Workspace>` reads `rootSplit`/`leftSplit`/`rightSplit` from the store, renders `<SidedockFrame side="left">` + `<RenderNode node={rootSplit}>` + `<SidedockFrame side="right">`.
  - `<SidedockFrame>` wraps `<RenderNode node={dock}>` with a collapse button and resize handle (reuse `shell/src/shell/ResizeHandle.tsx`). When `dock.collapsed`, render only the ribbon icons — same behavior as the current left/right panels.
  - `<RenderNode>` switches on `node.kind`: `'split'` → flex container with `node.sizes`; `'tabs'` → `<TabGroup>`; `'root'`/`'floating'` → recurse into `node.child`.
  - `<TabGroup>` renders `<TabStrip>` + `<LeafHost leaf={activeLeaf}>`.
  - `<LeafHost>` — the **one place a View's DOM lives**. A `<div ref={ref}>` whose element is assigned to `leaf.containerEl` in `useEffect`, then `leaf.view.onOpen(el)` is called. On unmount or leaf change, `leaf.view.onClose()`. React never re-renders inside this div — view owns its DOM imperatively. Treat it exactly like a CodeMirror mount point.

**Critical**: `<LeafHost>` must be stable across tab switches if possible. When the active tab changes, the old leaf's `onClose` runs and the new leaf's `onOpen` runs, but the inactive leaves' DOM should remain (hidden with `display:none`) so switching back is instant. Match Obsidian's behavior: switching between two markdown tabs preserves editor state. Implementation: keep one `<LeafHost>` per leaf in the tab group, not just the active one; toggle `display` based on `activeIndex`. Unload leaves only on `detach`.

**Exit criteria:** Render a fixture workspace with one editor leaf in root + file-explorer in left dock + outline in right dock. Switch tabs; verify old view's `onClose` fires only on detach, not on deactivation.

### Phase 5 — Wrap existing plugins as Views

Each current sidebar panel becomes a thin `View` subclass whose `onOpen(el)` mounts the existing React component via `createRoot(el).render(<ExistingComponent/>)`. Keep the existing plugin logic untouched.

**Views to create** (list derived from `shell/src/plugins/nexus/` — verify against actual tree):

| viewType | Wraps | Default host |
|---|---|---|
| `file-explorer` | `shell/src/plugins/nexus/files/*` | left |
| `search` | `shell/src/plugins/nexus/search/*` | left |
| `outline` | `shell/src/plugins/nexus/outline/*` | right |
| `backlink` | `shell/src/plugins/nexus/backlinks/*` | right |
| `graph` | `shell/src/plugins/nexus/graph/*` | main or right |
| `markdown` | editor plugin (center; holds file state) | main |
| `terminal` | `shell/src/plugins/nexus/terminal/*` | main (tabbed) |
| `ai-chat` | `shell/src/plugins/nexus/ai/*` | right or paneMode |
| `mcp` | `shell/src/plugins/nexus/mcp/*` | right |
| `skills` | `shell/src/plugins/nexus/skills/*` | right |
| `workflow` | `shell/src/plugins/nexus/workflow/*` | right |
| `empty` | built-in placeholder | any |

For each: create `shell/src/plugins/nexus/<plugin>/<Plugin>View.ts` that extends `ViewBase`, implements `getState`/`setState` to round-trip the plugin's own state (selected file, query string, active tab, etc.), and in `onOpen` mounts the React tree. In the plugin's `index.ts` registration, call:

```ts
viewRegistry.register('file-explorer', leaf => new FileExplorerView(leaf))
// in plugin onload — existence only, no reveal (see resolved decision #2):
await workspace.ensureLeafOfType('file-explorer', 'left')

// At a later call site that actually wants to show the pane (e.g. the
// "Focus File Explorer" command handler), compose with revealLeaf:
//   const leaf = await workspace.ensureLeafOfType('file-explorer', 'left')
//   workspace.revealLeaf(leaf)
```

Replace any `slotRegistry.register('sidebar', { component: FileExplorer, ... })` call with the pair above.

**Keep using `SlotRegistry`** for items that contribute to `titleBar`, `activityBar`, `statusBarLeft`, `statusBarRight`, `overlay`, `paneMode`. Those are chrome, not panes.

**Exit criteria:** Every current sidebar/editor panel renders via a View. Nothing user-visible changed. Manual smoke: click every activity-bar icon, verify the corresponding pane appears.

### Phase 6 — Layout persistence

**Files:**

- `shell/src/workspace/persistence.ts` — debounced (250ms) write of `workspaceStore.serialize()` to `<vault>/.forge/workspace.json` via the kernel bridge (`api.kernel.invoke('storage.write_vault_file', ...)` or the equivalent command — check `shell/src/shell/App.tsx` and `shell-kernel-bridge-plan.md` for the right surface).
- Boot sequence in `shell/src/shell/App.tsx`:
  1. Load `workspace.json` from vault. If absent/corrupt, use default layout.
  2. Register all core Views (Phase 5) — must happen **before** hydrate so `setViewState` creators resolve.
  3. `await workspaceStore.hydrate(json)`.
  4. Emit `layout-ready`.
- Subscribe to `layout-change`, `view-changed`, `active-leaf-change`, `pinned-change` → trigger debounced save.

**Persistence shape** (match Obsidian for forward compatibility):
```json
{
  "main":  { "id": "...", "type": "split", "children": [...], "direction": "horizontal" },
  "left":  { "id": "...", "type": "split", "children": [...], "collapsed": false, "size": 300 },
  "right": { "id": "...", "type": "split", "children": [...], "collapsed": false, "size": 300 },
  "active": "leaf-id",
  "lastOpenFiles": ["..."]
}
```

**Exit criteria:** Open the app, arrange panels, close specific files, reload. Layout + per-leaf state restored. Delete `workspace.json`; app boots with default layout.

### Phase 7 — Cleanup

- Remove now-unused `SlotId` values from `shell/src/registry/SlotRegistry.ts`: `sidebar`, `editorArea`, `editorTabs`, `panelArea`, `rightPanel`, `sidebarContent`, `panelAreaContent`, `rightPanelContent`.
- Keep: `overlay`, `titleBar`, `activityBar`, `statusBarLeft`, `statusBarRight`, `paneMode`.
- Delete any dead `slotRegistry.register(...)` calls for removed slot IDs.
- Update `shell/src/types/plugin.ts` — the plugin API surface should now expose `workspace: WorkspaceStore`, `viewRegistry`, `slotRegistry` as separate concerns.
- Add `docs/leaf-architecture.md` documenting the final shape (Leaf, View, ViewRegistry, setViewState, ensureSideLeaf, persistence format) for future plugin authors. Link from `README.md`.

**Exit criteria:** `rg -n "slotRegistry.register\('(sidebar|editorArea|rightPanel|panelArea)'` returns zero hits. `tsc` clean. Full manual smoke test passes.

## Out of scope (follow-up tickets)

Open these as separate issues after the migration lands; do not attempt in this effort:

- **Drag-and-drop between tab strips and between regions.** Now cheap to implement: `parent.remove(leaf); newParent.insert(leaf)`. Matches `docs/10-editor-shell.md:189-194`.
- **Popout windows** (`FloatingWindow`). Requires Tauri multi-window work.
- **Linked panes / groups** (`group` field on ViewState). Already in the type model; wire up scroll/history linking later.
- **`canDropAnywhere` flag on views** for release-notes-style panes that can cross region boundaries. Obsidian `release-notes` sets this (`docs/10-editor-shell.md:121`).
- **History stack per leaf** (back/forward navigation). Obsidian's `history` field on ViewState supports this.

## Risks

- **Mounting React subtrees inside imperatively-managed DOM is fiddly.** Use `createRoot(el)` in `onOpen`, call `root.unmount()` in `onClose`. Do not share roots between views. If a view wants hot reload, it owns the logic inside its own root — not our problem.
- **State serialization.** Each plugin's `getState`/`setState` must round-trip without data loss, including refs to `TFile` / path strings. Test hydrate after every Phase 5 wrapping.
- **Event ordering during hydrate.** `layout-ready` must fire *after* every leaf's `setViewState` completes — don't emit it early. Plugins depending on `layout-ready` (e.g. cursor restore) will race if this is wrong.
- **`contextIsolation` / `nodeIntegration`.** Nexus uses Tauri, not Electron, so this isn't a concern here — but note that Obsidian's model assumes full Node access in views. Nexus plugins must call the kernel via `api.kernel.invoke` instead (`shell-kernel-bridge-plan.md`). Don't port Obsidian plugin code verbatim; it will try to `require('fs')`.

## Resolved decisions

1. **Persistence path**: `<vault>/.forge/workspace.json`. Not `.obsidian/` (collides with Obsidian installs on the same vault) and not `.nexus/` (old name). Every other shell-owned config file should follow the same `.forge/` convention.

2. **Split existence from visibility.** Diverge from Obsidian's `ensureSideLeaf` (which bundles "create if missing" + "move to this side" + "reveal" into one call). Nexus ships two functions, each doing one job:
   - `ensureLeafOfType(type, side)` — existence guarantee only. If a leaf of this type exists anywhere, return it unchanged — do *not* move it. If none exists, create one in the named sidedock.
   - `revealLeaf(leaf)` — visibility only. Expand the containing sidedock if collapsed, activate the tab, set it active.

   Rationale: each function has an obvious contract from its name. Plugins that want "make this pane visible to the user" compose the two; plugins that want "create in background without stealing focus" (e.g. index-maintenance panes) only call the first. Avoids the move-or-create ambiguity entirely — `ensureLeafOfType` never moves anything, so users' layout choices are never overridden.

   Cost: plugin `onload` code becomes two lines instead of one. Acceptable.

3. **`SlotRegistry` stays for chrome only.** After this migration, `SlotRegistry` serves fixed chrome positions (`titleBar`, `activityBar`, `statusBarLeft`, `statusBarRight`, `overlay`, `paneMode`) — not movable panes. `ViewRegistry` + `Leaf` owns panes. Do not unify the two registries; their semantics are genuinely different (chrome items don't move, don't serialize per-instance state, don't have lifecycle).

## References

- `/home/baileyrd/projects/obsidian_reverse/docs/10-editor-shell.md` — primary source for all behavioral claims in this plan.
- `/home/baileyrd/projects/obsidian_reverse/docs/07-plugin-api.md §4.1` — full list of workspace events.
- `/home/baileyrd/projects/obsidian_reverse/docs/09-microkernel.md §4-§5` — `Workspace` + `WorkspaceLeaf` in the kernel class map.
- `/home/baileyrd/projects/obsidian_reverse/docs/06-data-models.md` — `workspace.json` on-disk shape.
- `shell/src/registry/SlotRegistry.ts` — current pane model; gets narrowed to chrome-only.
- `docs/shell-kernel-bridge-plan.md` — current shell ↔ kernel bridge; leaf migration is shell-only and does not affect it.
