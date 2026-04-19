import { create } from 'zustand'

export interface ActivityBarItem {
  id: string
  pluginId: string
  icon: string
  title: string
  viewId: string
  priority: number
}

interface ActivityBarState {
  items: ActivityBarItem[]
  activeViewId: string | null
  addItem: (item: ActivityBarItem) => void
  removeItem: (id: string) => void
  setActive: (viewId: string | null) => void
}

export const useActivityBarStore = create<ActivityBarState>((set) => ({
  items: [],
  activeViewId: null,
  addItem: (item) =>
    set((s) => ({
      items: [...s.items.filter((i) => i.id !== item.id), item].sort(
        (a, b) => a.priority - b.priority,
      ),
    })),
  removeItem: (id) =>
    set((s) => ({ items: s.items.filter((i) => i.id !== id) })),
  setActive: (viewId) => set({ activeViewId: viewId }),
}))
