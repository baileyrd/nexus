import { create } from 'zustand'

/**
 * Transient UI state for the quick-switcher overlay. Mirrors
 * commandPalette/commandPaletteStore.ts exactly — the file list itself is
 * NOT held here; it's re-pulled from `query_files` each time the modal
 * opens so files created/renamed elsewhere show up without a manual
 * refresh.
 */
export interface QuickSwitcherState {
  visible: boolean
  query: string
  selectedIndex: number

  open(): void
  close(): void
  setQuery(q: string): void
  setSelectedIndex(i: number): void
  moveSelection(delta: 1 | -1, resultsLen: number): void
}

export const useQuickSwitcherStore = create<QuickSwitcherState>((set) => ({
  visible: false,
  query: '',
  selectedIndex: 0,

  open: () => set({ visible: true, query: '', selectedIndex: 0 }),
  close: () => set({ visible: false }),

  setQuery: (q) => set({ query: q, selectedIndex: 0 }),
  setSelectedIndex: (i) => set({ selectedIndex: i }),

  moveSelection: (delta, resultsLen) =>
    set((s) => {
      if (resultsLen <= 0) return { selectedIndex: 0 }
      const next = s.selectedIndex + delta
      const clamped = Math.max(0, Math.min(resultsLen - 1, next))
      return { selectedIndex: clamped }
    }),
}))
