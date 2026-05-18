# Slot System

The slot system is how plugins render UI into the shell. The shell defines named regions (slots). Plugins register React components into those regions. The shell renders whatever has been registered — nothing more.

---

## The Overlay Slot — Special Case

The overlay slot is the most important slot to understand because it handles all floating UI: modals, dialogs, command palettes, notification toasts.

```css
/* The overlay container */
.shell-overlay {
  position: fixed;
  inset: 0;              /* covers the entire window */
  z-index: 9999;
  pointer-events: none;  /* transparent to clicks when empty */
}
```

The `pointer-events: none` is critical. When no overlay component is rendering visible content, mouse clicks pass straight through to the layout beneath. Individual overlay components set `pointer-events: auto` on their own elements when visible.

### Always-mounted pattern

Overlay components are registered once at plugin load time and stay mounted permanently. They render `null` when not visible:

```typescript
function CommandPaletteView() {
  const visible = useContextKey('commandPaletteVisible')
  if (!visible) return null
  return <div className="palette-backdrop"> ... </div>
}
```

**Why always-mounted?**
- No mounting delay when first opened
- State (scroll position, last query, etc.) preserved between opens
- No portal injection needed — component is already in the tree
- Simpler lifecycle — no mount/unmount events to handle

---

## App.tsx — The Slot Surface

App.tsx renders only slot surfaces. Nothing is hardcoded:

```typescript
// src/shell/App.tsx

import { useSlotStore } from '../registry/SlotRegistry'
import { SlotSurface } from './slots/SlotSurface'

export default function App() {
  const slots = useSlotStore(s => s.slots)

  return (
    <div className="shell-root">

      {/* Overlay — always last in DOM, always on top via z-index */}
      <div className="shell-overlay">
        <SlotSurface entries={slots.overlay} />
      </div>

      {/* Title bar */}
      <div className="shell-titlebar">
        <SlotSurface entries={slots.titleBar} />
      </div>

      {/* Body — activity bar + sidebar + center */}
      <div className="shell-body">
        <div className="shell-activitybar">
          <SlotSurface entries={slots.activityBar} />
        </div>

        <div className="shell-sidebar-region">
          <SlotSurface entries={slots.sidebar} />
        </div>

        <div className="shell-center">
          <SlotSurface entries={slots.editorArea} />
          <SlotSurface entries={slots.panelArea} />
        </div>
      </div>

      {/* Status bar */}
      <div className="shell-statusbar">
        <div className="shell-statusbar-left">
          <SlotSurface entries={slots.statusBarLeft} />
        </div>
        <div className="shell-statusbar-right">
          <SlotSurface entries={slots.statusBarRight} />
        </div>
      </div>

    </div>
  )
}
```

---

## SlotSurface Component

```typescript
// src/shell/slots/SlotSurface.tsx

import type { SlotEntry } from '../../registry/SlotRegistry'

interface Props {
  entries: SlotEntry[]
}

export function SlotSurface({ entries }: Props) {
  // Empty = renders nothing. This is correct and expected.
  return (
    <>
      {entries.map(entry => (
        <entry.component key={entry.id} />
      ))}
    </>
  )
}
```

Exactly this simple. No conditional logic. No fallbacks. Empty slot = empty DOM. The shell trusts that plugins will populate the slots they've declared.

---

## How a Plugin Registers into a Slot

Via the plugin API in `activate()`:

```typescript
activate(api: PluginAPI) {
  api.views.register('my-plugin.my-view', {
    slot: 'sidebar',
    component: MyViewComponent,
    priority: 30,
  })
}
```

Under the hood this calls:

```typescript
useSlotStore.getState().register('sidebar', {
  id: 'my-plugin.my-view',
  pluginId: 'my-org.my-plugin',
  component: MyViewComponent,
  priority: 30,
})
```

The Zustand store update triggers a re-render in `App.tsx` → `SlotSurface` → `MyViewComponent` mounts.

---

## Priority and Ordering

Entries within a slot are sorted by `priority` (ascending). Lower priority number = rendered first (or at the top/left, depending on the slot's flex direction).

```
priority: 10  → activity bar icon at the very top
priority: 20  → second icon
priority: 50  → default (middle of the pack)
priority: 90  → near the bottom
priority: 100 → overlay items (high priority = higher z-stack)
```

Plugins should use priority values with gaps (10, 20, 30 rather than 1, 2, 3) to leave room for other plugins to insert themselves.

---

## Shell Layout CSS

The layout regions are CSS flex containers. The slot surfaces inside them expand to fill:

```css
.shell-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
  background: var(--color-shell-background);
}

.shell-titlebar {
  flex-shrink: 0;
  height: 36px;
}

.shell-body {
  display: flex;
  flex: 1;
  overflow: hidden;
}

.shell-activitybar {
  flex-shrink: 0;
  width: 48px;
  display: flex;
  flex-direction: column;
}

.shell-sidebar-region {
  flex-shrink: 0;
  overflow: hidden;
  /* Width driven by layoutStore.sidebar.width via inline style */
}

.shell-center {
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow: hidden;
}

.shell-statusbar {
  flex-shrink: 0;
  height: 24px;
  display: flex;
  justify-content: space-between;
}

.shell-overlay {
  position: fixed;
  inset: 0;
  z-index: 9999;
  pointer-events: none;
}
```

---

## Resize Handles

The sidebar and panel area are resizable. Resize handles are rendered by the layout store's shell wrapper, not by the plugins themselves:

```typescript
// src/shell/App.tsx (expanded)

const { sidebar, panelArea } = useLayoutStore()

// In the body:
{sidebar.visible && (
  <>
    <div
      className="shell-sidebar-region"
      style={{ width: sidebar.width }}
    >
      <SlotSurface entries={slots.sidebar} />
    </div>
    <ResizeHandle
      direction="horizontal"
      onResize={w => useLayoutStore.getState().resizeSidebar(w)}
    />
  </>
)}
```

The resize handle is shell infrastructure, not a plugin contribution. Plugins don't need to know their container is resizable — they just fill whatever space they're given.

---

## Multiple Components in One Slot

A slot can have multiple entries. All of them render. For example, multiple status bar items in `statusBarLeft` render side by side. For `overlay`, multiple modals can be registered — each manages its own visibility and z-order via priority.

For exclusive slots (like `editorArea` — normally only one editor surface), plugins should coordinate via context keys or events rather than assuming they're the only entry. If two plugins both register into `editorArea` at priority 50, both render.

---

## Empty Shell Proof

Boot sequence with zero plugins:

```typescript
const plugins: Plugin[] = []
await host.loadAll(plugins)
ReactDOM.createRoot(document.getElementById('root')!).render(<App />)
```

Result:
- `slots.overlay` = `[]` → renders nothing
- `slots.titleBar` = `[]` → renders nothing
- `slots.activityBar` = `[]` → renders nothing
- `slots.sidebar` = `[]` → renders nothing
- `slots.editorArea` = `[]` → renders nothing
- `slots.panelArea` = `[]` → renders nothing
- `slots.statusBarLeft` = `[]` → renders nothing
- `slots.statusBarRight` = `[]` → renders nothing

You see a window with the shell's background color. No errors. No fallback content. This is correct.
