// src/plugins/nexus/sidebar/sidebarSplitStore.ts
// Obsidian-faithful sidebar leaf (tab) model for the left split.
// Each leaf is {id, type} where `type` matches a sidebarContent viewId.
// Multiple leaves of different types can coexist; revealLeaf finds-or-
// creates by type so repeated activity-bar clicks are idempotent.

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export interface SidebarLeaf {
  /** Session-unique id, `${type}-${counter}`. */
  id: string
  /** View id — matches a registered sidebarContent slot entry. */
  type: string
}

interface SidebarSplitState {
  leaves: SidebarLeaf[]
  activeLeafId: string | null

  /** Append a leaf of `type` and activate it. Returns the new leaf id. */
  addLeaf: (type: string) => string
  /** Remove leaf `id`. If it was active, fall back to the previous sibling
   *  (or the first remaining leaf if none). */
  removeLeaf: (id: string) => void
  /** Activate an existing leaf by id. No-op if not found. */
  setActiveLeaf: (id: string) => void
  /** Find a leaf of `type` and activate it; otherwise create one. Returns
   *  the leaf id (either existing or freshly minted). */
  revealLeaf: (type: string) => string
  /** Hard reset — drops every leaf. Used by boot recovery. */
  reset: () => void
}

// Session-scoped monotonic counter. IDs don't need cross-reload continuity —
// persist will rehydrate whatever leaf ids were in state last time, and new
// leaves created this session just won't collide with those (they carry the
// prior counter position in the id string itself).
let COUNTER = 0
function mintId(type: string): string {
  return `${type}-${++COUNTER}`
}

export const useSidebarSplitStore = create<SidebarSplitState>()(
  persist(
    (set, get) => ({
      leaves: [],
      activeLeafId: null,

      addLeaf: (type) => {
        const id = mintId(type)
        set((s) => ({
          leaves: [...s.leaves, { id, type }],
          activeLeafId: id,
        }))
        return id
      },

      removeLeaf: (id) => {
        const { leaves, activeLeafId } = get()
        const idx = leaves.findIndex((l) => l.id === id)
        if (idx === -1) return
        const next = leaves.filter((l) => l.id !== id)
        let nextActive = activeLeafId
        if (activeLeafId === id) {
          // Prefer the previous sibling; else the first remaining; else null.
          const fallback = next[idx - 1] ?? next[0] ?? null
          nextActive = fallback?.id ?? null
        }
        set({ leaves: next, activeLeafId: nextActive })
      },

      setActiveLeaf: (id) => {
        const exists = get().leaves.some((l) => l.id === id)
        if (!exists) return
        set({ activeLeafId: id })
      },

      revealLeaf: (type) => {
        const existing = get().leaves.find((l) => l.type === type)
        if (existing) {
          set({ activeLeafId: existing.id })
          return existing.id
        }
        return get().addLeaf(type)
      },

      reset: () => set({ leaves: [], activeLeafId: null }),
    }),
    {
      name: 'shell-sidebar-split',
      version: 1,
      // Persisted state wins — current state holds fresh action closures
      // and that's all we'd pull from it.
      merge: (persisted, current) => ({
        ...current,
        ...(persisted as Partial<SidebarSplitState>),
      }),
    },
  ),
)
