// src/registry/StatusBarRegistry.ts
// Manages status bar items contributed by plugins.
//
// Items can render either plain text (`text`) or a React node
// (`content`). When both are set, `content` wins — `text` stays as
// a convenience + accessible/tooltip fallback.

import type { ReactNode } from 'react'
import { create } from 'zustand'
import {
  resolveEffectivePriority,
  subscribePriorityChanges,
} from './priorityOverrides'

export interface StatusBarItem {
  id: string
  pluginId: string
  slot: 'left' | 'right'
  priority: number
  /** Plain-text label. Used when `content` is not provided, and as the
   *  default tooltip / aria-label. */
  text?: string
  /** Rich React content — dots, <code> badges, icons, etc. Takes
   *  precedence over `text` when both are set. */
  content?: ReactNode
  tooltip?: string
  command?: string
  /** Extra class names appended to the `.status-bar-item` root. Use
   *  `'ember'` to mark an accent-colored sync indicator, etc. */
  className?: string
  /** P2-02 — plugin's declared priority preserved across override
   *  resolution. */
  originalPriority?: number
}

interface StatusBarStore {
  items: StatusBarItem[]
  upsert: (item: StatusBarItem) => void
  update: (id: string, updates: Partial<StatusBarItem>) => void
  remove: (id: string) => void
  /** P2-02 — re-resolve override priorities + re-sort. */
  refreshPriorities: () => void
}

export const useStatusBarStore = create<StatusBarStore>((set) => ({
  items: [],

  upsert: (item) =>
    set(s => {
      const original = item.originalPriority ?? item.priority
      const effective = resolveEffectivePriority('statusBar', item.id, original)
      const next: StatusBarItem = { ...item, priority: effective, originalPriority: original }
      return {
        items: (s.items.some(i => i.id === next.id)
          ? s.items.map(i => i.id === next.id ? next : i)
          : [...s.items, next]
        ).sort((a, b) => a.priority - b.priority),
      }
    }),

  update: (id, updates) =>
    set(s => ({
      items: s.items.map(i => i.id === id ? { ...i, ...updates } : i),
    })),

  remove: (id) =>
    set(s => ({ items: s.items.filter(i => i.id !== id) })),

  refreshPriorities: () =>
    set(s => ({
      items: s.items
        .map(i => {
          const original = i.originalPriority ?? i.priority
          const effective = resolveEffectivePriority('statusBar', i.id, original)
          return effective === i.priority ? i : { ...i, priority: effective }
        })
        .sort((a, b) => a.priority - b.priority),
    })),
}))

subscribePriorityChanges('statusBar', () => {
  useStatusBarStore.getState().refreshPriorities()
})

export class StatusBarRegistry {
  create(pluginId: string, config: Omit<StatusBarItem, 'pluginId'>): {
    set text(v: string)
    set content(v: ReactNode)
    set tooltip(v: string)
    dispose(): void
  } {
    const item: StatusBarItem = { ...config, pluginId }
    useStatusBarStore.getState().upsert(item)

    return {
      set text(v: string) {
        useStatusBarStore.getState().update(config.id, { text: v })
      },
      set content(v: ReactNode) {
        useStatusBarStore.getState().update(config.id, { content: v })
      },
      set tooltip(v: string) {
        useStatusBarStore.getState().update(config.id, { tooltip: v })
      },
      dispose() {
        useStatusBarStore.getState().remove(config.id)
      },
    }
  }

  unregister(id: string) {
    useStatusBarStore.getState().remove(id)
  }
}
