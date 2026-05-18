# Workspace Layout & View Persistence

How the shell persists layout, where views live, and the chrome-vs-content split.

---

## Persistence model

One `workspace.json` per vault at `.nexus/workspace.json`. It stores **layout**, not content.

- **Layout persistence** — one file, Obsidian-style: which panes are open, split sizes, active tab, sidebar widths.
- **Domain artifacts** — many files, content-addressed: process definitions, agent runs, notes. Views render these; they don't own them.

Rule of thumb: `workspace.json` holds just enough state to *re-open* every view. Content comes from domain stores and files on disk.

### Schema

```jsonc
{
  "version": 1,
  "main":  { "type": "split", "direction": "horizontal", "children": [...] },
  "left":  { "type": "split", "children": [...], "width": 300, "collapsed": false },
  "right": { "type": "split", "children": [...], "width": 320, "collapsed": false },
  "bottom": { "height": 240, "activeTab": "terminal", "tabs": ["terminal","processes"] },
  "activityBar": { "pinned": ["files","search","templates","agent"] },
  "active": "<leafId>"
}
```

A leaf is a discriminated union — one variant per view type:

```jsonc
{ "type": "editor",    "path": "Nexus_Work/Status.md", "cursor": {"line":214,"col":33}, "scroll": 0, "mode": "live" }
{ "type": "processes", "selected": "nexus-app", "tab": "logs", "filter": "", "follow": true }
{ "type": "templates", "category": "all", "query": "", "scroll": 0 }
{ "type": "agent-run", "runId": "f4e8b2", "tab": "agents", "selectedAgent": "audit-finder" }
{ "type": "plan-dag",  "runId": "f4e8b2", "selectedStep": "extract-findings", "zoom": 1.0, "pan": [0,0] }
```

### Per-surface state files

Some surfaces need more than layout. They get their own file next to `workspace.json`:

| File | Purpose |
|---|---|
| `workspace.json` | layout tree + open tabs |
| `processes.json` | process definitions, restart counts, env (live logs stay in-memory / ring buffer) |
| `runs/<runId>.json` | each agent run is an artifact; Agent + Plan-DAG views both read one |

Templates need no file — the gallery is a view over a directory + plugin registry.

---

## Chrome vs content

**Chrome** = the UI frame around the content: titlebar, activity bar, sidebar frames, tab strips, status bar, resize handles. The scaffolding that's always there.

**Content** = what's *inside* a leaf: the editor, log stream, DAG graph, template gallery.

Quick test: if every document closed and every process stopped, what's left on screen? That's chrome.

|  | Chrome | Content |
|---|---|---|
| Lives in | fixed slots (or sidebar trees) | the split tree (leaves) |
| Owned by | `core/` plugins | `nexus/` plugins |
| Persisted as | `left` / `right` / `bottom` / `activityBar` keys | `main` tree + `LeafView` |
| User can drag/split? | no | yes |
| Usually one of | each | many |

### Obsidian blurs this line

In Obsidian the **left and right sidebars are themselves split trees containing leaves** — File Explorer, Outline, Backlinks are leaves, not bespoke panels. Only the ribbon and statusbar are pure chrome. The center `.mod-root` is just "the split whose leaves can't be dragged to a sidebar."

If going for pixel-parity with Obsidian, use **three leaf trees** (`main`, `left`, `right`) plus ribbon/statusbar chrome — same leaf type system everywhere, sidebars become a place leaves can *live* rather than a fixed slot.

---

## What sets sizes and boundaries

Three sizing layers, each owned by someone different:

| Layer | Sets size | Source of truth |
|---|---|---|
| Outer shell (titlebar height, ribbon width, statusbar height) | CSS in the workbench stylesheet | design tokens — fixed, not user-resizable |
| Sidebar / panel extents (left width, right width, bottom height) | `ResizeHandle` → `workspace.json` siblings | persisted, user-draggable |
| Leaf sizes inside a split tree | `ResizeHandle` → `split.children[i].size` (or inline `flex-grow`) | persisted, user-draggable |

The boundary between chrome and content is literally a CSS boundary — the grid cell that holds the center `<LayoutTree>`. Everything outside that cell is chrome; everything inside is the leaf tree.

### Root layout sketch

```tsx
<div className="workbench">   {/* CSS grid: rows=titlebar/body/statusbar, cols=activity/left/center/right */}
  <TitleBarSlot />
  <ActivityBarSlot />
  <LeftSidebarSlot width={left.width}>
    <LayoutTree root={workspace.left} />
  </LeftSidebarSlot>
  <CenterSlot>
    <LayoutTree root={workspace.main} />
    <BottomPanelSlot height={bottom.height} />
  </CenterSlot>
  <RightSidebarSlot width={right.width}>
    <LayoutTree root={workspace.right} />
  </RightSidebarSlot>
  <StatusBarSlot />
</div>
```

Consequences:

1. Chrome can't be dragged into the center, and leaves can't escape into chrome — different DOM subtrees with different parents. Split/drag code only operates on nodes under a `<LayoutTree>`.
2. Collapsing a sidebar = `left.collapsed = true` (CSS width → 0). The slot stays mounted so its plugin keeps running.

### Obsidian reference

Obsidian is closed-source but the DOM is observable:

```
.app-container
└── .horizontal-main-container
    └── .workspace                          ← root layout
        ├── .workspace-ribbon.mod-left      ← activity bar (chrome)
        ├── .workspace-split.mod-root       ← CENTER: leaf tree
        │   └── .workspace-tabs / .workspace-split (recursive)
        │       └── .workspace-leaf         ← leaves
        ├── .workspace-split.mod-left-split    ← left sidebar (chrome + leaves)
        ├── .workspace-split.mod-right-split   ← right sidebar (chrome + leaves)
        └── .status-bar                     ← statusbar (chrome)
```

Where sizing comes from:

| Thing | Where |
|---|---|
| Ribbon width, statusbar height, titlebar height | `app.css` — fixed pixel values |
| Sidebar widths | inline `style="width: 300px"`, written by resize handle, persisted to `workspace.json` |
| Leaf splits inside a tree | inline `style="flex-grow: N"` on each `.workspace-split` child |
| Collapsed sidebar | `.is-collapsed` class toggle + CSS width 0 |

See also [obsidian/obsidian-runtime.md](obsidian/obsidian-runtime.md) and [obsidian/obsidian-measurements.md](obsidian/obsidian-measurements.md).

---

## Where views live

Leaves are **contributed by plugins**, not hardcoded in the shell. The layout tree itself is owned by the shell.

```
shell/core         owns  →  the tree, splits, resize, drag-drop, persistence
plugin (nexus/*)   owns  →  view types + their renderers + their state
```

### Plugin contribution shape

```ts
// plugins/nexus/agent/index.ts
export default definePlugin({
  id: 'nexus.agent',
  views: [
    {
      type: 'agent-run',           // matches LeafView.type in workspace.json
      title: 'Agent Run',
      icon: SparkleIcon,
      component: AgentRunView,     // React component, props: { state: LeafState }
      serialize:   (s) => ({...}), // state → JSON for workspace.json
      deserialize: (json) => ({...}),
    },
    { type: 'plan-dag', component: PlanDagView, ... },
  ],
})
```

The shell's editor area / `SlotRegistry`:

1. Reads `workspace.json`, walks the tree.
2. For each leaf, looks up `type` in the view registry.
3. Mounts the plugin's component with the leaf's state.

### Why it matters

- **Deactivate a plugin → its leaves render a placeholder** ("this view requires `nexus.agent`"), layout survives.
- **Community plugins can add new leaf types** without forking the shell.
- **Shell code never imports domain code** — one-way dependency, same pattern as VS Code's `IEditorPaneRegistry`.

### Plugin → view mapping for the five reference views

| View | Plugin | Persistence |
|---|---|---|
| Editor | `core/editorArea` *(exists)* | leaf.state only; file is source of truth |
| Processes / logs | `nexus/processes` *(exists, extend)* | `processes.json` for defs; logs = in-memory ring |
| Templates | `nexus/templates` *(new)* | none — view over `templates/` dir + registry |
| Agent run | `nexus/agent` *(exists, extend)* | `runs/<runId>.json` per run |
| Plan DAG | same `nexus/agent` plugin, second view | reads same run artifact |

Agent-run and Plan-DAG are **two views over one artifact**, not two plugins. One plugin registers both view types, both read the same `runStore`.

---

## Implementation order

1. **Layout tree first** — replace flat `editorStore.tabs` with a split/leaf tree; migrate the editor to leaf variant `type:"editor"`. Nothing else works right until this lands.
2. **Persistence** — load/save `workspace.json` on boot/shutdown + debounced on change. Same plumbing then serves every future view.
3. **Run artifact schema** — `runs/<id>.json` before building agent UI. The view is a renderer, so nail the data shape first.
4. **Agent + Plan-DAG views** — both read `runStore.get(runId)`, different renderers.
5. **Templates** — last. Simplest: read-only gallery, no persistence.

The load-bearing decision is step 1. Once leaves are a discriminated union, adding view #6 / #7 later is a 50-line PR instead of a refactor.
