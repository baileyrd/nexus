import { create } from 'zustand'
import {
  resolveEffectivePriority,
  subscribePriorityChanges,
} from '../../../registry/priorityOverrides'

export interface ActivityBarItem {
  id: string
  pluginId: string
  icon: string
  title: string
  viewId: string
  priority: number
  /** P2-02 — plugin's declared priority, preserved across override
   *  resolution so live re-sorts can recompute against the same
   *  baseline. */
  originalPriority?: number
}

interface ActivityBarStore {
  items: ActivityBarItem[]
  activeId: string | null
  addItem: (item: ActivityBarItem) => void
  removeItem: (id: string) => void
  setActive: (id: string | null) => void
  /** P2-02 — re-resolve every item's effective priority and re-sort. */
  refreshPriorities: () => void
}

export const useActivityBarStore = create<ActivityBarStore>((set) => ({
  items: [],
  activeId: null,
  addItem: (item) =>
    set(s => {
      const original = item.originalPriority ?? item.priority
      const effective = resolveEffectivePriority('activityBar', item.id, original)
      const next: ActivityBarItem = {
        ...item,
        priority: effective,
        originalPriority: original,
      }
      return {
        items: [...s.items.filter(i => i.id !== next.id), next]
          .sort((a, b) => a.priority - b.priority),
      }
    }),
  removeItem: (id) => set(s => ({ items: s.items.filter(i => i.id !== id) })),
  setActive: (id) => set({ activeId: id }),
  refreshPriorities: () =>
    set(s => ({
      items: s.items
        .map(i => {
          const original = i.originalPriority ?? i.priority
          const effective = resolveEffectivePriority('activityBar', i.id, original)
          return effective === i.priority ? i : { ...i, priority: effective }
        })
        .sort((a, b) => a.priority - b.priority),
    })),
}))

subscribePriorityChanges('activityBar', () => {
  useActivityBarStore.getState().refreshPriorities()
})
