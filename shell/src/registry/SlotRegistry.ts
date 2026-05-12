// src/registry/SlotRegistry.ts
// Maps slot IDs to sorted lists of plugin-contributed components.
// Backed by Zustand so slot changes trigger React re-renders immediately.
//
// After the Leaf + ViewRegistry migration (see docs/architecture/leaf-architecture.md
// and docs/archive/leaf-migration-plan.md §Phase 7), `SlotRegistry` is restricted to
// *chrome* positions only — fixed regions that don't move, don't persist
// per-instance state, and don't participate in the tabbed/movable pane
// model. Movable panes go through `workspace` + `viewRegistry`.

import type { ComponentType } from 'react'
import { create } from 'zustand'
import type { SlotId } from '@nexus/extension-api'

// OI-04 — `SlotId` is declared in `@nexus/extension-api` so native and
// community plugins see identical slot names at the contract boundary.
// Re-exported here so in-tree imports of `SlotId` from this module keep
// working; the former pane slots (`sidebar`, `editorArea`, `editorTabs`,
// `panelArea`, `rightPanel`, `sidebarContent`, `panelAreaContent`,
// `rightPanelContent`) were removed in Phase 7 of the leaf migration —
// use `viewRegistry.register(type, creator)` plus
// `workspace.ensureLeafOfType(type, side)` instead.
export type { SlotId }

export interface SlotEntry {
  id: string
  pluginId: string
  // Heterogeneous component registry — see PluginAPI.ts for rationale.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  component: ComponentType<any>
  priority: number
}

interface SlotStore {
  slots: Record<SlotId, SlotEntry[]>
  register: (slotId: SlotId, entry: SlotEntry) => void
  unregister: (entryId: string) => void
}

export const useSlotStore = create<SlotStore>((set) => ({
  slots: {
    overlay: [],
    titleBar: [],
    activityBar: [],
    statusBarLeft: [],
    statusBarRight: [],
    paneMode: [],
  },

  register: (slotId, entry) =>
    set(s => ({
      slots: {
        ...s.slots,
        [slotId]: [...s.slots[slotId], entry]
          .sort((a, b) => a.priority - b.priority),
      },
    })),

  unregister: (entryId) =>
    set(s => ({
      slots: Object.fromEntries(
        Object.entries(s.slots).map(([k, entries]) => [
          k,
          (entries as SlotEntry[]).filter(e => e.id !== entryId),
        ])
      ) as Record<SlotId, SlotEntry[]>,
    })),
}))

// Non-reactive read — for use outside React (e.g., in the extension host)
export const slotRegistry = {
  register: (slotId: SlotId, entry: SlotEntry) =>
    useSlotStore.getState().register(slotId, entry),

  unregister: (entryId: string) =>
    useSlotStore.getState().unregister(entryId),
}
