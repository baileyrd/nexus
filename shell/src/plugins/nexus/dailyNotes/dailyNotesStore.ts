import { create } from 'zustand'

/**
 * Tracks the date the "previous"/"next" navigation commands step
 * relative to — the last daily note this plugin opened (via "Open
 * today", a calendar click, or a prior prev/next). `null` until the
 * first open, at which point prev/next fall back to today.
 */
interface DailyNotesState {
  currentDate: string | null // ISO YYYY-MM-DD
  setCurrentDate(iso: string): void
}

export const useDailyNotesStore = create<DailyNotesState>((set) => ({
  currentDate: null,
  setCurrentDate: (iso) => set({ currentDate: iso }),
}))
