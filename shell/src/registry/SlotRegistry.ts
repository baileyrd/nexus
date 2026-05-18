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
import {
  resolveEffectivePriority,
  subscribePriorityChanges,
} from './priorityOverrides'

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
  /** P2-02 — the plugin's declared priority. `priority` above may
   *  have been overridden by `nexus.priority.slot.<id>`; this field
   *  preserves the contributed default so live overrides can be
   *  recomputed against the same baseline. */
  originalPriority?: number
}

interface SlotStore {
  /** Keyed by `SlotId` for the built-in slots plus any plugin-defined
   *  slot identifiers added at runtime via `defineSlot`. Stored as
   *  `Record<string, …>` so dynamic ids don't require widening the
   *  `SlotId` contract. */
  slots: Record<string, SlotEntry[]>
  register: (slotId: string, entry: SlotEntry) => void
  unregister: (entryId: string) => void
  /** P4-10 — register an empty slot identifier. Idempotent: calling
   *  `defineSlot('x')` repeatedly leaves prior contributions intact. */
  defineSlot: (slotId: string) => void
  /** P2-02 — re-resolve every entry's effective priority against the
   *  current configStore and re-sort. Called by the priority-override
   *  subscriber when `nexus.priority.slot.*` changes. */
  refreshPriorities: () => void
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

  register: (slotId, entry) => {
    // P2-02 — let `nexus.priority.slot.<entryId>` override the
    // contributed priority. Keep the declared value on
    // `originalPriority` so live overrides can recompute against it.
    const original = entry.originalPriority ?? entry.priority
    const effective = resolveEffectivePriority('slot', entry.id, original)
    const next: SlotEntry = { ...entry, priority: effective, originalPriority: original }
    set(s => ({
      slots: {
        ...s.slots,
        // Auto-define on first registration so a plugin can
        // register straight into a slot it just defined.
        [slotId]: [...(s.slots[slotId] ?? []), next]
          .sort((a, b) => a.priority - b.priority),
      },
    }))
  },

  defineSlot: (slotId) =>
    set(s =>
      s.slots[slotId] !== undefined ? s : { slots: { ...s.slots, [slotId]: [] } },
    ),

  unregister: (entryId) =>
    set(s => ({
      slots: Object.fromEntries(
        Object.entries(s.slots).map(([k, entries]) => [
          k,
          (entries as SlotEntry[]).filter(e => e.id !== entryId),
        ])
      ) as Record<SlotId, SlotEntry[]>,
    })),

  refreshPriorities: () =>
    set(s => ({
      slots: Object.fromEntries(
        Object.entries(s.slots).map(([k, entries]) => {
          const rebuilt = (entries as SlotEntry[])
            .map(e => {
              const original = e.originalPriority ?? e.priority
              const effective = resolveEffectivePriority('slot', e.id, original)
              return effective === e.priority ? e : { ...e, priority: effective }
            })
            .sort((a, b) => a.priority - b.priority)
          return [k, rebuilt]
        }),
      ) as Record<string, SlotEntry[]>,
    })),
}))

// P2-02 — refresh slot ordering whenever any `nexus.priority.slot.*`
// changes. Tear-down isn't tied to React lifecycle because the slot
// registry is a module singleton; the subscription lives for the
// process lifetime.
subscribePriorityChanges('slot', () => {
  useSlotStore.getState().refreshPriorities()
})

/**
 * BL-067 Phase 0 — JSON-safe snapshot of a [`SlotEntry`].
 *
 * The React component reference is intentionally dropped because
 * React `ComponentType`s are not serialisable and the View Builder
 * surface only needs identity (`id` + `pluginId`) + ordering
 * (`priority`) to render its inventory + composition palette.
 */
export interface SlotEntrySnapshot {
  /** Stable entry id assigned by the caller at register time. */
  id: string
  /** Reverse-DNS id of the registering plugin. */
  pluginId: string
  /** Sort key inside the slot (lower = earlier). */
  priority: number
}

/**
 * BL-067 Phase 0 — JSON-safe snapshot of every slot's contributions.
 * Keyed by `SlotId`; every slot is present even when empty so the
 * View Builder can iterate the chrome positions without checking
 * for `undefined` first.
 */
export type SlotInventory = Record<SlotId, SlotEntrySnapshot[]>

// Non-reactive read — for use outside React (e.g., in the extension host)
export const slotRegistry = {
  register: (slotId: SlotId | string, entry: SlotEntry) =>
    useSlotStore.getState().register(slotId, entry),

  unregister: (entryId: string) =>
    useSlotStore.getState().unregister(entryId),

  /** P4-10 — declare a slot identifier so plugin contributions can
   *  target it before any registration has happened. Idempotent. */
  defineSlot: (slotId: string) => useSlotStore.getState().defineSlot(slotId),

  /**
   * BL-067 Phase 0 — pure snapshot of every slot's entries minus
   * the React component reference. Safe to surface to plugins; the
   * View Builder feeds this into its composition canvas.
   */
  snapshot(): SlotInventory {
    const raw = useSlotStore.getState().slots
    const out = {} as SlotInventory
    for (const slot of Object.keys(raw) as SlotId[]) {
      out[slot] = (raw[slot] ?? []).map((entry) => ({
        id: entry.id,
        pluginId: entry.pluginId,
        priority: entry.priority,
      }))
    }
    return out
  },
}
