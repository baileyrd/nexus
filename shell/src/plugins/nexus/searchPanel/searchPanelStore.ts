// shell/src/plugins/nexus/searchPanel/searchPanelStore.ts
//
// BL-078 — view-model for the cross-forge find / replace panel.
//
// Wraps `com.nexus.storage::find_in_files` and `replace_in_files`.
// Holds query state, the current result set, loading / error
// flags, and per-file expanded state for the result tree. The
// store is intentionally flat — the panel UI is small and a single
// Zustand store keeps the action surface easy to reason about.
//
// Authoritative shape lives in
// `crates/nexus-storage/src/find_replace.rs` (`FindInFilesArgs`,
// `FileMatches`, `LineMatch`, `ReplaceInFilesArgs`,
// `ReplaceReport`). Field naming mirrors the serde JSON form so
// rows round-trip through the IPC layer without a transform.

import { create } from 'zustand'

const PLUGIN_ID = 'com.nexus.storage'
const CMD_FIND = 'find_in_files'
const CMD_REPLACE = 'replace_in_files'

/** Mirror of `crates/nexus-storage/src/find_replace.rs::LineMatch`. */
export interface LineMatch {
  line: number
  column: number
  length: number
  text: string
  before?: string | null
  after?: string | null
}

/** Mirror of `crates/nexus-storage/src/find_replace.rs::FileMatches`. */
export interface FileMatches {
  relpath: string
  hits: LineMatch[]
}

/** Mirror of `crates/nexus-storage/src/find_replace.rs::ReplaceError`. */
export interface ReplaceError {
  relpath: string
  message: string
}

/** Mirror of `crates/nexus-storage/src/find_replace.rs::ReplaceReport`. */
export interface ReplaceReport {
  files_changed: number
  replacements_applied: number
  errors?: ReplaceError[]
}

/** Subset of PluginAPI we depend on; structurally typed so a unit
 *  test can mock `invoke` without fabricating a full PluginAPI. */
export interface SearchKernelAPI {
  invoke<T = unknown>(
    pluginId: string,
    commandId: string,
    args?: unknown,
  ): Promise<T>
  available?(): Promise<boolean>
}

interface SearchPanelState {
  // ── inputs ──────────────────────────────────────────────────
  query: string
  replacement: string
  isRegex: boolean
  caseSensitive: boolean
  wholeWord: boolean

  // ── results ─────────────────────────────────────────────────
  results: FileMatches[]
  /** True once a search has completed at least once for the
   *  current input. Cleared when the query changes so the empty
   *  state renders "type to search" rather than "no matches". */
  searched: boolean
  /** True while a query is in flight. */
  loading: boolean
  /** True while a replace is in flight. */
  replacing: boolean
  /** Last surfaced error from either find or replace. Cleared on
   *  the next successful run. */
  error: string | null
  /** Per-file expanded flag for the result tree. Default: every
   *  group expanded after a fresh search; the user can collapse
   *  individual groups via the disclosure caret. */
  expanded: Record<string, boolean>
  /** Last-applied replace report (or `null` if none yet). The
   *  panel renders a single-line summary under the toolbar after
   *  a successful apply. */
  lastReplace: ReplaceReport | null

  // ── input setters ───────────────────────────────────────────
  setQuery(q: string): void
  setReplacement(r: string): void
  setIsRegex(v: boolean): void
  setCaseSensitive(v: boolean): void
  setWholeWord(v: boolean): void
  toggleExpanded(relpath: string): void

  // ── actions ─────────────────────────────────────────────────
  /** Run a search with the current input. Replaces the result
   *  list when the call resolves; does nothing when the query is
   *  empty / whitespace. */
  runSearch(api: SearchKernelAPI): Promise<void>
  /** Apply the current replacement against `restrict` (or every
   *  matching file when `null`). Re-runs the search after a
   *  successful apply so the result list reflects post-replace
   *  state. */
  applyReplace(api: SearchKernelAPI, restrict: string[] | null): Promise<void>
  /** Drop every input + result so the next mount starts fresh. */
  reset(): void
}

const INITIAL = {
  query: '',
  replacement: '',
  isRegex: false,
  caseSensitive: false,
  wholeWord: false,
  results: [] as FileMatches[],
  searched: false,
  loading: false,
  replacing: false,
  error: null as string | null,
  expanded: {} as Record<string, boolean>,
  lastReplace: null as ReplaceReport | null,
}

export const useSearchPanelStore = create<SearchPanelState>((set, get) => ({
  ...INITIAL,

  setQuery: (q) => set({ query: q, searched: false, error: null }),
  setReplacement: (r) => set({ replacement: r }),
  setIsRegex: (v) => set({ isRegex: v, searched: false }),
  setCaseSensitive: (v) => set({ caseSensitive: v, searched: false }),
  setWholeWord: (v) => set({ wholeWord: v, searched: false }),
  toggleExpanded: (relpath) =>
    set((s) => ({
      expanded: { ...s.expanded, [relpath]: !(s.expanded[relpath] ?? true) },
    })),

  runSearch: async (api) => {
    const { query, isRegex, caseSensitive, wholeWord } = get()
    const trimmed = query.trim()
    if (!trimmed) {
      set({ results: [], searched: false, error: null })
      return
    }
    if (api.available && !(await api.available())) {
      set({ error: 'Storage backend not available', loading: false })
      return
    }
    set({ loading: true, error: null })
    try {
      const res = await api.invoke<FileMatches[]>(PLUGIN_ID, CMD_FIND, {
        query,
        is_regex: isRegex,
        case_sensitive: caseSensitive,
        whole_word: wholeWord,
      })
      const expanded: Record<string, boolean> = {}
      for (const file of res ?? []) expanded[file.relpath] = true
      set({
        results: res ?? [],
        searched: true,
        loading: false,
        expanded,
        lastReplace: null,
      })
    } catch (err) {
      set({ error: String(err), loading: false, searched: true, results: [] })
    }
  },

  applyReplace: async (api, restrict) => {
    const { query, replacement, isRegex, caseSensitive, wholeWord } = get()
    if (!query.trim()) return
    set({ replacing: true, error: null })
    try {
      const report = await api.invoke<ReplaceReport>(PLUGIN_ID, CMD_REPLACE, {
        query,
        replacement,
        is_regex: isRegex,
        case_sensitive: caseSensitive,
        whole_word: wholeWord,
        files: restrict ?? null,
      })
      set({ replacing: false, lastReplace: report })
      // Re-run the search so the result list reflects the
      // post-replace state. If the replacement removed every hit
      // (typical), the list will be empty; if some files now
      // contain a partial new substring (e.g. user replaced
      // `foo` with `foobar`), those rows reappear.
      await get().runSearch(api)
    } catch (err) {
      set({ replacing: false, error: String(err) })
    }
  },

  reset: () => set({ ...INITIAL }),
}))
