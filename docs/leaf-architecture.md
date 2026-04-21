# Leaf + ViewRegistry Architecture

The Nexus shell separates **chrome** (fixed title/activity/status bar
positions) from **panes** (tabbed, movable, persistable regions).
Panes are modelled on Obsidian's `WorkspaceLeaf` / `ViewRegistry`
primitives; chrome still goes through the narrow `SlotRegistry`. This
document is the quick reference for plugin authors and shell
contributors. The full rationale lives in
[`docs/leaf-migration-plan.md`](./leaf-migration-plan.md).

## Primitives

### `Leaf`
A single tabbed pane. Carries an id, a `parent` pointer into the
workspace tree, a live `View` instance, a `containerEl` (assigned by
`<LeafHost>` in React), and the usual metadata (`pinned`, `group`,
`active`). Defined in `shell/src/workspace/types.ts`; the concrete
implementation is `LeafImpl` in `shell/src/workspace/Leaf.ts`.

### `View`
The pane's contents. Every view has a string `viewType` that matches
a registration key in the `ViewRegistry` and four optional lifecycle
hooks: `getState`, `setState`, `onOpen(containerEl)`, `onClose()`.
`ViewBase` (`shell/src/workspace/View.ts`) supplies noop defaults.

### `ViewRegistry`
Maps `viewType → creator`, where a creator is `(leaf: Leaf) => View`.
Also maps file extensions to view types (`viewRegistry.registerExtensions(['md'], 'markdown')`).
A built-in `empty` creator is always registered so a persisted layout
that references an unregistered type still boots.

### `WorkspaceStore`
The live layout tree (`rootSplit`, `leftSplit`, `rightSplit`,
`floating`), the leaf registry, the internal event bus, and the
`setViewState` choke-point. Exposed as the non-reactive `workspace`
facade from `shell/src/workspace/workspaceStore.ts`.

## `setViewState`: the one mutation path

Every state change — opening a file, switching a tab's type, hydrating
from disk, drag-dropping a pane (future) — reduces to
`leaf.setViewState({ type, state?, active?, pinned?, group? })`.
The algorithm (`Leaf.ts`) follows Obsidian's:

1. `await prevView.onClose()` (if any), clear the container element.
2. Look up `creator = viewRegistry.getCreator(state.type) ?? empty`.
3. Instantiate `view = creator(leaf)`.
4. If `state.state` is provided, `await view.setState(state.state, eState)`.
5. If the leaf is mounted, `await view.onOpen(containerEl)`.
6. Apply `pinned`/`group`; emit `view-changed`; if `state.active`, bridge
   to `setActiveLeaf`.

Serialization, drag-and-drop, and popouts (future) all go through this
single entry point. Don't reach around it.

## Existence vs visibility

Nexus deliberately splits Obsidian's `ensureSideLeaf` into two
functions (resolved decision #2 in the migration plan):

- **`workspace.ensureLeafOfType(type, side)`** — existence only. If any
  leaf of this type already exists anywhere in the workspace, returns
  it **unchanged** (does not move it). Otherwise creates a new leaf in
  the named sidedock and `setViewState({type})`s it.

- **`workspace.revealLeaf(leaf)`** — visibility only. Expands the
  containing sidedock if collapsed, activates the tab, sets it active.

Compose for the common "open this pane for the user" intent:

```ts
const leaf = await workspace.ensureLeafOfType('file-explorer', 'left')
workspace.revealLeaf(leaf)
```

Plugins that want "register a background pane without stealing focus"
(e.g. an indexer) call only `ensureLeafOfType`. The user's layout
choices are never overridden by a plugin's boot code.

## Persistence

The layout is written to `<vault>/.forge/workspace.json` with a 250 ms
debounce on every `layout-change` / `view-changed` /
`active-leaf-change` / `pinned-change` event. Shape matches Obsidian's
for forward compatibility:

```json
{
  "main":  { "id": "...", "type": "split",  "children": [...], "direction": "horizontal" },
  "left":  { "id": "...", "type": "split",  "children": [...], "collapsed": false, "size": 300 },
  "right": { "id": "...", "type": "split",  "children": [...], "collapsed": false, "size": 300 },
  "active": "leaf-id",
  "lastOpenFiles": ["..."]
}
```

Boot sequence (`shell/src/shell/App.tsx`): load `workspace.json`,
ensure every plugin's `viewRegistry.register(...)` has run, then
`await workspaceStore.hydrate(json)`. A missing or corrupt file falls
through to `buildDefaultLayout()`.

## Writing a plugin View

```ts
import { createRoot, type Root } from 'react-dom/client'
import { ViewBase, viewRegistry, workspace, type Leaf } from '@/workspace'

class OutlinePaneView extends ViewBase {
  readonly viewType = 'outline'
  private root: Root | null = null

  async onOpen(el: HTMLElement): Promise<void> {
    this.root = createRoot(el)
    this.root.render(<OutlineView />)
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

// In your plugin's activate(api):
viewRegistry.register('outline', (leaf) => new OutlinePaneView(leaf))

api.commands.register('myplugin.outline.focus', async () => {
  const leaf = await workspace.ensureLeafOfType('outline', 'right')
  workspace.revealLeaf(leaf)
})
```

Plugins can reach the workspace facade via `api.workspace` /
`api.viewRegistry` instead of importing `@/workspace` directly.

## The remaining role of `SlotRegistry`

After Phase 7, `SlotRegistry` serves *chrome* only:

- `overlay` — command palette, modals, toasts.
- `titleBar` — custom title bar content.
- `activityBar` — the left-edge icon strip.
- `statusBarLeft` / `statusBarRight` — fixed status-bar segments.
- `paneMode` — full-window takeover for agent/workflow modal UIs.

Panes (`sidebar`, `editorArea`, `panelArea`, `rightPanel`, and the
`*Content` variants) are gone. If you're reaching for `slot:` on one
of those strings, you want `viewRegistry.register` +
`workspace.ensureLeafOfType` instead.
