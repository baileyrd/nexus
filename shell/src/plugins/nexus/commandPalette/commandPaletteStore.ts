import { create } from 'zustand'

/**
 * Tiny store that owns the palette's transient UI state. The list of
 * registered commands is NOT held here — we re-pull it from
 * `api.commands.all()` each time the modal opens so late-loading
 * plugins show up without an explicit refresh.
 */
export interface CommandPaletteState {
  visible: boolean
  query: string
  /** 0-based index into the currently-filtered results list. */
  selectedIndex: number

  open(): void
  close(): void
  /** Sets the query and clamps selectedIndex back to 0 so a
   *  shortened result set never leaves the cursor off the end. */
  setQuery(q: string): void
  setSelectedIndex(i: number): void
  /** Move the selection by `delta`, clamped to `[0, resultsLen - 1]`.
   *  Caller passes the current results length so the store doesn't
   *  need to know about filtering. */
  moveSelection(delta: 1 | -1, resultsLen: number): void
}

export const useCommandPaletteStore = create<CommandPaletteState>((set) => ({
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
