import { create } from 'zustand'

/**
 * Metadata the rightPanel host needs per contributed tab, beyond what
 * the SlotEntry itself carries. `viewId` matches the SlotEntry.id
 * under the `rightPanelContent` slot; the host looks up the component
 * by that id when rendering. `priority` is mirrored from the view
 * registration so the tab row can sort independently of the slot's
 * own priority order.
 */
export interface RightPanelTabMeta {
  title: string
  priority: number
}

interface RightPanelState {
  /** viewId → { title, priority } */
  tabs: Record<string, RightPanelTabMeta>
  /** The currently-selected tab's viewId, or null when none registered. */
  activeViewId: string | null

  /**
   * Register (or replace) metadata for a tab. The first registered
   * tab auto-activates when nothing is active yet, so whichever
   * plugin fires `rightPanel:registerTab` first becomes the default.
   */
  registerTab: (viewId: string, meta: RightPanelTabMeta) => void
  /** Drop a tab's metadata. If it was active, active falls back to null. */
  unregisterTab: (viewId: string) => void
  /** Explicit activation — typically from a tab click. */
  setActive: (viewId: string) => void
}

export const useRightPanelStore = create<RightPanelState>((set, get) => ({
  tabs: {},
  activeViewId: null,

  registerTab: (viewId, meta) =>
    set((s) => ({
      tabs: { ...s.tabs, [viewId]: meta },
      // First-registered-wins: auto-select when nothing active yet.
      activeViewId: s.activeViewId ?? viewId,
    })),

  unregisterTab: (viewId) =>
    set((s) => {
      const { [viewId]: _, ...rest } = s.tabs
      const nextActive = s.activeViewId === viewId ? null : s.activeViewId
      return { tabs: rest, activeViewId: nextActive }
    }),

  setActive: (viewId) => {
    if (get().tabs[viewId]) set({ activeViewId: viewId })
  },
}))
