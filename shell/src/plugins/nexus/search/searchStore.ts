import { create } from 'zustand'

/**
 * One hit in the result list.
 *
 * Decoded from `com.nexus.storage::search`'s `SearchResult` (see
 * crates/nexus-storage/src/search.rs::SearchResult — `file_path`,
 * `block_id`, `block_type`, `excerpt`, `score`). We keep only the
 * fields the sidebar UI actually renders; block_id / block_type are
 * dropped because a click on a hit opens the file, not the specific
 * block, and the shell has no block-level navigation yet.
 *
 * `relpath` is forge-relative, forward-slash separated — matches the
 * `file_path` string the kernel emits and is what `files:open`
 * consumers already expect.
 *
 * `snippet` is populated by Tantivy's SnippetGenerator on the kernel
 * side (plain text, no highlight markup — see search.rs where `<b>`
 * tags are stripped). May be empty when the snippet generator can't
 * produce one; the UI skips the snippet row in that case.
 */
export interface SearchHit {
  relpath: string
  snippet: string
  score: number
}

interface SearchState {
  query: string
  results: SearchHit[]
  loading: boolean
  error: string | null
  /** 0-based index into `results`. Clamped to 0 when results shrink. */
  selectedIndex: number
  setQuery(q: string): void
  setResults(hits: SearchHit[]): void
  setLoading(b: boolean): void
  setError(e: string | null): void
  setSelectedIndex(i: number): void
  reset(): void
}

export const useSearchStore = create<SearchState>((set) => ({
  query: '',
  results: [],
  loading: false,
  error: null,
  selectedIndex: 0,

  setQuery: (q) => set({ query: q, selectedIndex: 0 }),
  setResults: (hits) =>
    set((s) => ({
      results: hits,
      selectedIndex: Math.min(s.selectedIndex, Math.max(0, hits.length - 1)),
    })),
  setLoading: (b) => set({ loading: b }),
  setError: (e) => set({ error: e }),
  setSelectedIndex: (i) => set({ selectedIndex: i }),
  reset: () =>
    set({
      query: '',
      results: [],
      loading: false,
      error: null,
      selectedIndex: 0,
    }),
}))
