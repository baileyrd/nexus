import { create } from 'zustand'
import type { IconName } from '../../../icons'

export interface ActivityBarItem {
  id: string
  pluginId: string
  icon: string
  /**
   * Optional SVG path `d` (viewBox 0 0 24 24, stroke-only). When set,
   * renders as an SVG instead of `icon` text. Single-path only —
   * multi-element icons (search, graph, sparkle, …) need `iconName`.
   * Kept for back-compat with items that hand-rolled a glyph before
   * the icon module landed.
   */
  iconPath?: string
  /**
   * Preferred for new items. Names a glyph in `shell/src/icons/` and
   * lets the activity bar render multi-element icons via the shared
   * `<Icon>` component. Wins over `iconPath` / `icon` when set.
   */
  iconName?: IconName
  title: string
  viewId: string
  priority: number
  /** Where in the bar to render the item. Defaults to 'top'. */
  placement?: 'top' | 'bottom'
  /**
   * If set, clicking the item executes this command ID instead of
   * toggling a sidebar view. Intended for action items (e.g. settings)
   * that live in the bottom cluster.
   */
  command?: string
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
