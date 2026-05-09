> **Archived 2026-04-26** — Documents the named-layout-presets feature, which was rejected in v1 per [ADR 0012](../../../docs/adr/0012-drop-named-layout-presets.md). The feature is not implemented; this doc is kept as a record of the original design intent.

# Predefined Layouts

Predefined layouts are named snapshots of the shell's spatial state. They let users (and plugins) switch between complete panel arrangements instantly.

---

## What a Layout Captures

A layout definition captures every dimension of the shell's visual state:

```typescript
interface LayoutDefinition {
  id: string
  name: string
  version: number           // for migration on schema changes

  panels: {
    sidebar: {
      visible: boolean
      width: number
      activeView: string    // which sidebar tab is active
    }
    panelArea: {
      visible: boolean
      height: number
      activePanel: string
    }
    rightPanel: {
      visible: boolean
      width: number
      activeView: string
    }
    activityBar: {
      visible: boolean
    }
    titleBar: {
      visible: boolean
    }
    statusBar: {
      visible: boolean
    }
  }

  editorArea: {
    splits: SplitNode[]     // the tile tree
    activeGroupId: string
  }
}
```

---

## The Layout Store

```typescript
// src/stores/layoutStore.ts
import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface LayoutStore {
  // Current live state
  sidebar: { visible: boolean; width: number; activeView: string }
  panelArea: { visible: boolean; height: number; activePanel: string }
  rightPanel: { visible: boolean; width: number }

  // Named saved layouts
  savedLayouts: Record<string, LayoutDefinition>

  // Actions
  toggleSidebar: () => void
  resizeSidebar: (width: number) => void
  togglePanelArea: () => void
  resizePanelArea: (height: number) => void

  // Layout management
  saveLayout: (id: string, name: string) => void
  applyLayout: (id: string) => void
  deleteLayout: (id: string) => void
  resetToDefault: () => void
}

export const useLayoutStore = create<LayoutStore>()(
  persist(
    (set, get) => ({
      sidebar: { visible: true, width: 260, activeView: 'fileExplorer' },
      panelArea: { visible: false, height: 200, activePanel: 'terminal' },
      rightPanel: { visible: false, width: 300 },

      savedLayouts: {
        // Built-in predefined layouts
        default: {
          id: 'default',
          name: 'Default',
          version: 1,
          panels: {
            sidebar: { visible: true, width: 260, activeView: 'fileExplorer' },
            panelArea: { visible: false, height: 200, activePanel: 'terminal' },
            rightPanel: { visible: false, width: 300 },
            activityBar: { visible: true },
            titleBar: { visible: true },
            statusBar: { visible: true },
          },
          editorArea: { splits: [], activeGroupId: 'main' },
        },
        zen: {
          id: 'zen',
          name: 'Zen Mode',
          version: 1,
          panels: {
            sidebar: { visible: false, width: 260, activeView: 'fileExplorer' },
            panelArea: { visible: false, height: 200, activePanel: 'terminal' },
            rightPanel: { visible: false, width: 300 },
            activityBar: { visible: false },
            titleBar: { visible: false },
            statusBar: { visible: false },
          },
          editorArea: { splits: [], activeGroupId: 'main' },
        },
        debug: {
          id: 'debug',
          name: 'Debug',
          version: 1,
          panels: {
            sidebar: { visible: true, width: 280, activeView: 'debugExplorer' },
            panelArea: { visible: true, height: 300, activePanel: 'debugConsole' },
            rightPanel: { visible: false, width: 300 },
            activityBar: { visible: true },
            titleBar: { visible: true },
            statusBar: { visible: true },
          },
          editorArea: { splits: [], activeGroupId: 'main' },
        },
      },

      toggleSidebar: () =>
        set(s => ({ sidebar: { ...s.sidebar, visible: !s.sidebar.visible } })),

      resizeSidebar: (width) =>
        set(s => ({ sidebar: { ...s.sidebar, width: Math.max(180, Math.min(600, width)) } })),

      togglePanelArea: () =>
        set(s => ({ panelArea: { ...s.panelArea, visible: !s.panelArea.visible } })),

      resizePanelArea: (height) =>
        set(s => ({ panelArea: { ...s.panelArea, height: Math.max(120, Math.min(600, height)) } })),

      saveLayout: (id, name) => {
        const s = get()
        const definition: LayoutDefinition = {
          id,
          name,
          version: 1,
          panels: {
            sidebar: s.sidebar,
            panelArea: s.panelArea,
            rightPanel: s.rightPanel,
            activityBar: { visible: true },
            titleBar: { visible: true },
            statusBar: { visible: true },
          },
          editorArea: { splits: [], activeGroupId: 'main' },
        }
        set(s => ({ savedLayouts: { ...s.savedLayouts, [id]: definition } }))
      },

      applyLayout: (id) => {
        const layout = get().savedLayouts[id]
        if (!layout) return

        set({
          sidebar: layout.panels.sidebar,
          panelArea: layout.panels.panelArea,
          rightPanel: layout.panels.rightPanel,
        })
      },

      deleteLayout: (id) => {
        // Don't allow deleting built-in layouts
        if (['default', 'zen', 'debug'].includes(id)) return
        set(s => {
          const { [id]: _, ...rest } = s.savedLayouts
          return { savedLayouts: rest }
        })
      },

      resetToDefault: () => {
        get().applyLayout('default')
      },
    }),
    { name: 'shell-layout' }
  )
)
```

---

## Built-in Layouts

### Default

The standard arrangement: sidebar visible, panel area hidden, activity bar visible.

### Zen Mode

Maximum focus: sidebar hidden, panel area hidden, activity bar hidden, status bar hidden. Just the editor.

Triggered by: `workbench.action.toggleZenMode` (`⌘K Z`)

### Debug

Sidebar shows the debug explorer, panel area shows the debug console at 300px.

Automatically triggered when a debug session starts (if the debug plugin is loaded).

---

## Switching Layouts

### Via command

```typescript
// Register the layout commands in activate()
api.commands.register('workbench.action.toggleZenMode', () => {
  const store = useLayoutStore.getState()
  const isZen = !store.sidebar.visible && !store.panelArea.visible
  store.applyLayout(isZen ? 'default' : 'zen')
})
```

### Via the layout picker

The settings panel can include a layout picker section contributed by `core.title-bar` or a dedicated layout manager plugin.

### Programmatically (from any plugin)

```typescript
useLayoutStore.getState().applyLayout('debug')
```

---

## Automatic Layout Switching

Plugins can trigger layout switches in response to events:

```typescript
// In a debug plugin's activate():
api.events.on('debug:sessionStarted', () => {
  useLayoutStore.getState().applyLayout('debug')
})

api.events.on('debug:sessionEnded', () => {
  useLayoutStore.getState().applyLayout('default')
})
```

This is the Xcode Behaviors model: layouts respond to events rather than requiring manual switching.

---

## Schema Migration

When the `LayoutDefinition` schema changes (new fields, renamed panels), stored layouts need migration:

```typescript
function migrateLayout(raw: Record<string, unknown>): LayoutDefinition {
  const version = (raw.version as number) ?? 1

  if (version < 2) {
    // v1 → v2: added rightPanel field
    raw.panels = {
      ...(raw.panels as Record<string, unknown>),
      rightPanel: raw.panels?.rightPanel ?? { visible: false, width: 300 },
    }
    raw.version = 2
  }

  if (version < 3) {
    // v2 → v3: added editorArea field
    raw.editorArea = raw.editorArea ?? { splits: [], activeGroupId: 'main' }
    raw.version = 3
  }

  return raw as LayoutDefinition
}
```

The Zustand `persist` middleware's `migrate` option runs this on load:

```typescript
persist(
  (set, get) => ({ ... }),
  {
    name: 'shell-layout',
    version: 3,
    migrate: (persisted, version) => migrateLayout(persisted as Record<string, unknown>),
  }
)
```

---

## Layout vs Plugin State

Layouts capture the shell's **spatial** state (which panels are open, their sizes, which views are active). They do not capture:

- Which files are open in the editor
- Terminal history
- Search results
- Any plugin's internal state

If a plugin wants to save and restore its internal state across sessions, it uses `api.storage` (per-plugin persistent key-value store), not the layout system.
