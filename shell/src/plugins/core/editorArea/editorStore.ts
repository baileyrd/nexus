import { create } from 'zustand'

export interface EditorTab {
  id: string
  path: string
  title: string
  isDirty: boolean
  isPinned: boolean
  isPreview: boolean
}

interface EditorStore {
  tabs: EditorTab[]
  activeTabId: string | null
  openTab: (tab: Omit<EditorTab, 'id'>) => string
  closeTab: (id: string) => void
  setActiveTab: (id: string) => void
  markDirty: (id: string, dirty: boolean) => void
  pinTab: (id: string) => void
}

export const useEditorStore = create<EditorStore>((set, get) => ({
  tabs: [],
  activeTabId: null,

  openTab: (tab) => {
    const existing = get().tabs.find(t => t.path === tab.path)
    if (existing) { set({ activeTabId: existing.id }); return existing.id }
    const id = Math.random().toString(36).slice(2)
    set(s => ({ tabs: [...s.tabs, { ...tab, id }], activeTabId: id }))
    return id
  },

  closeTab: (id) =>
    set(s => {
      const idx = s.tabs.findIndex(t => t.id === id)
      const remaining = s.tabs.filter(t => t.id !== id)
      const newActive = s.activeTabId === id
        ? (remaining[idx] ?? remaining[idx - 1])?.id ?? null
        : s.activeTabId
      return { tabs: remaining, activeTabId: newActive }
    }),

  setActiveTab: (id) => set({ activeTabId: id }),
  markDirty: (id, dirty) => set(s => ({ tabs: s.tabs.map(t => t.id === id ? { ...t, isDirty: dirty } : t) })),
  pinTab: (id) => set(s => ({ tabs: s.tabs.map(t => t.id === id ? { ...t, isPinned: !t.isPinned, isPreview: false } : t) })),
}))
