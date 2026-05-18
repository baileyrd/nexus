import { create } from 'zustand'
import {
  resolveEffectivePriority,
  subscribePriorityChanges,
} from '../../../registry/priorityOverrides'

export interface PanelTab {
  id: string
  title: string
  pluginId: string
  priority: number
  /** P2-02 — plugin's declared priority preserved for live overrides. */
  originalPriority?: number
}

interface PanelAreaStore {
  tabs: PanelTab[]
  activeTabId: string | null
  registerTab: (tab: PanelTab) => void
  unregisterTab: (id: string) => void
  setActiveTab: (id: string) => void
  /** P2-02 — re-resolve override priorities + re-sort. */
  refreshPriorities: () => void
}

export const usePanelAreaStore = create<PanelAreaStore>((set) => ({
  tabs: [],
  activeTabId: null,
  registerTab: (tab) =>
    set(s => {
      const original = tab.originalPriority ?? tab.priority
      const effective = resolveEffectivePriority('panelArea', tab.id, original)
      const next: PanelTab = { ...tab, priority: effective, originalPriority: original }
      return {
        tabs: [...s.tabs.filter(t => t.id !== next.id), next].sort((a, b) => a.priority - b.priority),
        activeTabId: s.activeTabId ?? next.id,
      }
    }),
  unregisterTab: (id) =>
    set(s => ({
      tabs: s.tabs.filter(t => t.id !== id),
      activeTabId: s.activeTabId === id ? (s.tabs.find(t => t.id !== id)?.id ?? null) : s.activeTabId,
    })),
  setActiveTab: (id) => set({ activeTabId: id }),
  refreshPriorities: () =>
    set(s => ({
      tabs: s.tabs
        .map(t => {
          const original = t.originalPriority ?? t.priority
          const effective = resolveEffectivePriority('panelArea', t.id, original)
          return effective === t.priority ? t : { ...t, priority: effective }
        })
        .sort((a, b) => a.priority - b.priority),
    })),
}))

subscribePriorityChanges('panelArea', () => {
  usePanelAreaStore.getState().refreshPriorities()
})
