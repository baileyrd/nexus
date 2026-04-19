// src/registry/SlotRegistry.ts
// Maps slot IDs to sorted lists of plugin-contributed components.
// Backed by Zustand so slot changes trigger React re-renders immediately.

import type { ComponentType } from 'react'
import { create } from 'zustand'

export type SlotId =
  | 'overlay'
  | 'titleBar'
  | 'activityBar'
  | 'sidebar'
  | 'editorArea'
  | 'editorTabs'
  | 'panelArea'
  | 'rightPanel'
  | 'statusBarLeft'
  | 'statusBarRight'
  | 'sidebarContent'
  | 'panelAreaContent'
  | 'rightPanelContent'

export interface SlotEntry {
  id: string
  pluginId: string
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
    sidebar: [],
    editorArea: [],
    editorTabs: [],
    panelArea: [],
    rightPanel: [],
    statusBarLeft: [],
    statusBarRight: [],
    sidebarContent: [],
    panelAreaContent: [],
    rightPanelContent: [],
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
