import { create } from 'zustand'

export interface PanelTab {
  id: string
  title: string
  pluginId: string
  priority: number
}

interface PanelAreaStore {
  tabs: PanelTab[]
  activeTabId: string | null
  registerTab: (tab: PanelTab) => void
  unregisterTab: (id: string) => void
  setActiveTab: (id: string) => void
}

export const usePanelAreaStore = create<PanelAreaStore>((set) => ({
  tabs: [],
  activeTabId: null,
  registerTab: (tab) =>
    set(s => ({
      tabs: [...s.tabs.filter(t => t.id !== tab.id), tab].sort((a, b) => a.priority - b.priority),
      activeTabId: s.activeTabId ?? tab.id,
    })),
  unregisterTab: (id) =>
    set(s => ({
      tabs: s.tabs.filter(t => t.id !== id),
      activeTabId: s.activeTabId === id ? (s.tabs.find(t => t.id !== id)?.id ?? null) : s.activeTabId,
    })),
  setActiveTab: (id) => set({ activeTabId: id }),
}))
