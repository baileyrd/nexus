import { create } from 'zustand'

export interface ActivityBarItem {
  id: string
  pluginId: string
  icon: string
  title: string
  viewId: string
  priority: number
}

interface ActivityBarStore {
  items: ActivityBarItem[]
  activeId: string | null
  addItem: (item: ActivityBarItem) => void
  removeItem: (id: string) => void
  setActive: (id: string | null) => void
}

export const useActivityBarStore = create<ActivityBarStore>((set) => ({
  items: [],
  activeId: null,
  addItem: (item) =>
    set(s => ({
      items: [...s.items.filter(i => i.id !== item.id), item]
        .sort((a, b) => a.priority - b.priority),
    })),
  removeItem: (id) => set(s => ({ items: s.items.filter(i => i.id !== id) })),
  setActive: (id) => set({ activeId: id }),
}))
