// shell/src/plugins/nexus/recall/recallStore.ts
//
// BL-044 — transient state for the MEM recall overlay.
//
// Mirrors the shape of `cmdIStore.ts` (BL-032) so the overlay component
// can reuse the same visual scaffold without coupling the two surfaces.
// Only UI-state lives here; the search machinery lives in the runtime.
//
// Lifecycle:
//
//   open()                       → visible: true; query/results cleared.
//   setQuery(text)               → records the latest input; the runtime
//                                  reads this on debounce-fire.
//   beginSearch(reqId)           → status: 'searching'; clears prior error.
//   setResults(reqId, matches)   → status: 'idle'; results replaced.
//                                  Late callbacks for stale reqIds are
//                                  dropped by the runtime, but the store
//                                  also clamps `selectedIndex` to the
//                                  results length to keep the UI safe.
//   setError(err)                → status: 'error'.
//   moveSelection(delta)         → arrow keys navigate selection,
//                                  clamped to [0, results.length-1].
//   close()                      → visible: false; query/results retained
//                                  so a re-open with the same hotkey
//                                  doesn't flicker between empty and
//                                  prior state — the runtime resets on
//                                  open() if it wants a clean slate.

import { create } from 'zustand'

export type RecallStatus = 'idle' | 'searching' | 'error'

/** Minimal projection of `ChunkMatch` (com.nexus.ai::semantic_search).
 *  Mirrors the shape decoded by the runtime so the store stays
 *  ChunkMatch-shaped without taking a backend type dep. */
export interface RecallMatch {
  file_path: string
  block_id?: number
  chunk_text: string
  score: number
}

export interface RecallState {
  visible: boolean
  query: string
  results: RecallMatch[]
  selectedIndex: number
  status: RecallStatus
  error: Error | null
  /** Correlation id for the active search; the runtime sets this in
   *  `beginSearch` and only commits results when the matching id is
   *  still current. */
  currentRequestId: string | null
  /** BL-046 phase 2 — when `true`, the overlay filters results to
   *  matches detected as code captures (BL-046 phase 1 `#code/`
   *  tag or fence header). Resets to `false` on `open()` so each
   *  hotkey press starts unfiltered; persisted across `close()`
   *  during a single overlay session for symmetry with `query`. */
  codeOnly: boolean

  open(): void
  close(): void
  setQuery(q: string): void
  beginSearch(requestId: string): void
  setResults(requestId: string, matches: RecallMatch[]): void
  setError(err: Error): void
  moveSelection(delta: number): void
  setSelectedIndex(idx: number): void
  /** Toggle the BL-046 code-only filter. Reclamps `selectedIndex`
   *  to stay within the visible result list. */
  setCodeOnly(value: boolean): void
}

const INITIAL: Pick<
  RecallState,
  | 'visible'
  | 'query'
  | 'results'
  | 'selectedIndex'
  | 'status'
  | 'error'
  | 'currentRequestId'
  | 'codeOnly'
> = {
  visible: false,
  query: '',
  results: [],
  selectedIndex: 0,
  status: 'idle',
  error: null,
  currentRequestId: null,
  codeOnly: false,
}

function clamp(idx: number, length: number): number {
  if (length <= 0) return 0
  if (idx < 0) return 0
  if (idx >= length) return length - 1
  return idx
}

export const useRecallStore = create<RecallState>((set, get) => ({
  ...INITIAL,

  open: () =>
    set({
      visible: true,
      query: '',
      results: [],
      selectedIndex: 0,
      status: 'idle',
      error: null,
      currentRequestId: null,
      codeOnly: false,
    }),

  close: () =>
    // Keep query/results around so reopening with the same hotkey
    // doesn't flash an empty state during the next open()'s reset.
    // The next open() wipes them anyway.
    set({ visible: false, currentRequestId: null }),

  setQuery: (q) => set({ query: q }),

  beginSearch: (requestId) =>
    set({
      status: 'searching',
      error: null,
      currentRequestId: requestId,
    }),

  setResults: (requestId, matches) => {
    const state = get()
    if (state.currentRequestId !== requestId) return // stale
    set({
      results: matches,
      status: 'idle',
      currentRequestId: null,
      selectedIndex: clamp(state.selectedIndex, matches.length),
    })
  },

  setError: (err) =>
    set({
      status: 'error',
      error: err,
      currentRequestId: null,
    }),

  moveSelection: (delta) => {
    const state = get()
    if (state.results.length === 0) {
      set({ selectedIndex: 0 })
      return
    }
    set({ selectedIndex: clamp(state.selectedIndex + delta, state.results.length) })
  },

  setSelectedIndex: (idx) => {
    const state = get()
    set({ selectedIndex: clamp(idx, state.results.length) })
  },

  setCodeOnly: (value) => {
    // The visible-result count changes when the chip toggles, so
    // reclamp the selection to keep the highlight in range. The
    // pure filter helper lives in `codeFilter.ts`; we replicate
    // the predicate inline here to avoid a circular import (the
    // filter module depends on this store's `RecallMatch` type).
    const state = get()
    const visibleCount = value
      ? state.results.filter((m) => /(^|\n)(#code\/|File:\s+\S+|```[a-zA-Z][\w+-]*)/.test(
          m.chunk_text ?? '',
        )).length
      : state.results.length
    set({
      codeOnly: value,
      selectedIndex: visibleCount > 0 ? Math.min(state.selectedIndex, visibleCount - 1) : 0,
    })
  },
}))
