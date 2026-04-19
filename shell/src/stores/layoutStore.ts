// src/stores/layoutStore.ts
// Shell spatial state: panel visibility, sizes, active views.
// Persisted to localStorage via Zustand persist middleware.

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export interface LayoutDefinition {
  id: string
  name: string
  version: number
  panels: {
    sidebar:    { visible: boolean; width: number; activeView: string }
    panelArea:  { visible: boolean; height: number; activePanel: string }
    rightPanel: { visible: boolean; width: number }
    activityBar:{ visible: boolean }
    statusBar:  { visible: boolean }
  }
}

interface LayoutStore {
  sidebar:    { visible: boolean; width: number; activeView: string }
  panelArea:  { visible: boolean; height: number; activePanel: string }
  rightPanel: { visible: boolean; width: number }
  activityBar:{ visible: boolean }
  statusBar:  { visible: boolean }

  savedLayouts: Record<string, LayoutDefinition>

  // Panel toggles
  toggleSidebar:    () => void
  togglePanelArea:  () => void
  toggleRightPanel: () => void

  // Panel resize (with min/max clamping)
  resizeSidebar:    (width: number)  => void
  resizePanelArea:  (height: number) => void
  resizeRightPanel: (width: number)  => void

  // View activation
  setActiveSidebarView:  (viewId: string) => void
  setActivePanel:        (panelId: string) => void

  // Named layouts
  saveLayout:    (id: string, name: string) => void
  applyLayout:   (id: string) => void
  deleteLayout:  (id: string) => void
  resetToDefault:() => void
}

const BUILTIN_LAYOUTS: Record<string, LayoutDefinition> = {
  default: {
    id: 'default', name: 'Default', version: 1,
    panels: {
      sidebar:     { visible: true,  width: 260,  activeView: 'fileExplorer' },
      panelArea:   { visible: false, height: 200, activePanel: 'terminal' },
      rightPanel:  { visible: true,  width: 300 },
      activityBar: { visible: true },
      statusBar:   { visible: true },
    },
  },
  zen: {
    id: 'zen', name: 'Zen Mode', version: 1,
    panels: {
      sidebar:     { visible: false, width: 260,  activeView: 'fileExplorer' },
      panelArea:   { visible: false, height: 200, activePanel: 'terminal' },
      rightPanel:  { visible: false, width: 300 },
      activityBar: { visible: false },
      statusBar:   { visible: false },
    },
  },
  debug: {
    id: 'debug', name: 'Debug', version: 1,
    panels: {
      sidebar:     { visible: true,  width: 280,  activeView: 'debugExplorer' },
      panelArea:   { visible: true,  height: 300, activePanel: 'debugConsole' },
      rightPanel:  { visible: false, width: 300 },
      activityBar: { visible: true },
      statusBar:   { visible: true },
    },
  },
}

export const useLayoutStore = create<LayoutStore>()(
  persist(
    (set, get) => ({
      sidebar:     { visible: true,  width: 260,  activeView: 'fileExplorer' },
      panelArea:   { visible: false, height: 200, activePanel: 'terminal' },
      rightPanel:  { visible: true,  width: 300 },
      activityBar: { visible: true },
      statusBar:   { visible: true },

      savedLayouts: { ...BUILTIN_LAYOUTS },

      toggleSidebar:    () => set(s => ({ sidebar:    { ...s.sidebar,    visible: !s.sidebar.visible } })),
      togglePanelArea:  () => set(s => ({ panelArea:  { ...s.panelArea,  visible: !s.panelArea.visible } })),
      toggleRightPanel: () => set(s => ({ rightPanel: { ...s.rightPanel, visible: !s.rightPanel.visible } })),

      resizeSidebar:    (w) => set(s => ({ sidebar:    { ...s.sidebar,    width:  clamp(w, 180, 600) } })),
      resizePanelArea:  (h) => set(s => ({ panelArea:  { ...s.panelArea,  height: clamp(h, 120, 600) } })),
      resizeRightPanel: (w) => set(s => ({ rightPanel: { ...s.rightPanel, width:  clamp(w, 200, 600) } })),

      setActiveSidebarView: (viewId) => set(s => ({ sidebar:   { ...s.sidebar,   activeView:  viewId } })),
      setActivePanel:       (panelId)=> set(s => ({ panelArea: { ...s.panelArea, activePanel: panelId } })),

      saveLayout: (id, name) => {
        const s = get()
        const def: LayoutDefinition = {
          id, name, version: 1,
          panels: {
            sidebar:     s.sidebar,
            panelArea:   s.panelArea,
            rightPanel:  s.rightPanel,
            activityBar: s.activityBar,
            statusBar:   s.statusBar,
          },
        }
        set(s => ({ savedLayouts: { ...s.savedLayouts, [id]: def } }))
      },

      applyLayout: (id) => {
        const layout = get().savedLayouts[id]
        if (!layout) {
          console.warn(`[LayoutStore] Layout '${id}' not found`)
          return
        }
        set({
          sidebar:     layout.panels.sidebar,
          panelArea:   layout.panels.panelArea,
          rightPanel:  layout.panels.rightPanel,
          activityBar: layout.panels.activityBar,
          statusBar:   layout.panels.statusBar,
        })
      },

      deleteLayout: (id) => {
        if (id in BUILTIN_LAYOUTS) {
          console.warn(`[LayoutStore] Cannot delete built-in layout '${id}'`)
          return
        }
        set(s => {
          const { [id]: _, ...rest } = s.savedLayouts
          return { savedLayouts: rest }
        })
      },

      resetToDefault: () => get().applyLayout('default'),
    }),
    {
      name: 'shell-layout',
      // v2: Forge migration — adds rightPanel default-visible, tokens,
      // density. Bumping the version resets any pre-migration cache.
      version: 2,
      // Merge persisted state with defaults rather than full replace
      merge: (persisted, current) => ({ ...current, ...(persisted as Partial<LayoutStore>) }),
    }
  )
)

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
