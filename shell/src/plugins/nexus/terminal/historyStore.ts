// shell/src/plugins/nexus/terminal/historyStore.ts
//
// BL-060 — view-model for the ad-hoc command-history sub-view of
// nexus.terminal. Wraps the kernel `com.nexus.terminal::adhoc_*`
// handlers (list / get / delete / promote) and caches the row list
// locally so React renders are synchronous.
//
// Authoritative shape lives in `crates/nexus-terminal/src/adhoc.rs`
// (`AdHocRecord`, `AdHocStatus`). Field names mirror serde's snake_case
// representation verbatim — no transform on the way in or out.
//
// Promote is an ad-hoc → saved-commands handoff: the user names a row
// and we call `adhoc_promote`, which constructs a `SavedCommand` and
// inserts it. After a successful promote we reload both lists so the
// caller's saved-commands cache also catches up.

import { create } from 'zustand'

const PLUGIN_ID = 'com.nexus.terminal'
const CMD_LIST = 'adhoc_list'
const CMD_DELETE = 'adhoc_delete'
const CMD_PROMOTE = 'adhoc_promote'

/** Mirror of `crates/nexus-terminal/src/adhoc.rs::AdHocStatus`. */
export type AdHocStatus = 'success' | 'failure' | 'timeout'

/** Mirror of `crates/nexus-terminal/src/adhoc.rs::AdHocRecord`. */
export interface AdHocRecord {
  id: string
  command: string
  working_dir: string | null
  /** Unix seconds. */
  executed_at: number
  /** Process exit code; `null` when killed before exit. */
  exit_code: number | null
  /** Run duration in milliseconds. */
  duration_ms: number
  /** Times this `(command, working_dir)` pair has run. */
  run_count: number
  status: AdHocStatus
}

/** Subset of PluginAPI we actually need; structurally typed so this
 *  store can be unit-tested with a minimal mock. */
export interface HistoryKernelAPI {
  invoke<T = unknown>(
    pluginId: string,
    commandId: string,
    args?: unknown,
  ): Promise<T>
}

interface HistoryState {
  /** Cache of `adhoc_list` output — already in `executed_at desc`. */
  rows: AdHocRecord[]
  /** True once `loadHistory` has completed at least once. */
  loaded: boolean
  /** True while the latest `loadHistory` is in flight. */
  loading: boolean
  /** Last error surfaced by a kernel call; cleared on the next success. */
  error: string | null

  loadHistory(api: HistoryKernelAPI, limit?: number): Promise<void>
  deleteHistory(api: HistoryKernelAPI, id: string): Promise<void>
  /**
   * Promote an ad-hoc row to a saved command. Returns the new
   * `slug` so the caller can navigate / focus the saved-commands
   * panel. Throws on collision so the caller can prompt for a new
   * name.
   */
  promoteHistory(
    api: HistoryKernelAPI,
    id: string,
    name: string,
    options?: { slug?: string; icon?: string; shell?: string },
  ): Promise<string>
  /** Clear cached state. Used by workspace:closed. */
  reset(): void
}

/** Default page size for the History pane. Matches the CLI default
 *  (`nexus proc history --limit 100`) so both surfaces show the same
 *  row count without explicit configuration. */
const DEFAULT_LIMIT = 100

export const useHistoryStore = create<HistoryState>((set, get) => ({
  rows: [],
  loaded: false,
  loading: false,
  error: null,

  loadHistory: async (api, limit = DEFAULT_LIMIT) => {
    if (get().loading) return
    set({ loading: true })
    try {
      const rows = await api.invoke<AdHocRecord[]>(PLUGIN_ID, CMD_LIST, { limit })
      set({ rows: rows ?? [], loaded: true, loading: false, error: null })
    } catch (err) {
      set({ loading: false, error: String(err) })
    }
  },

  deleteHistory: async (api, id) => {
    try {
      await api.invoke(PLUGIN_ID, CMD_DELETE, { id })
      // Optimistic prune so the row vanishes before the refresh
      // round-trip lands. The refresh below re-syncs from truth.
      set((s) => ({ rows: s.rows.filter((r) => r.id !== id) }))
      await get().loadHistory(api)
    } catch (err) {
      set({ error: String(err) })
      throw err
    }
  },

  promoteHistory: async (api, id, name, options) => {
    const args: Record<string, unknown> = { id, name }
    if (options?.slug) args.slug = options.slug
    if (options?.icon) args.icon = options.icon
    if (options?.shell) args.shell = options.shell
    try {
      const saved = await api.invoke<{ slug: string }>(PLUGIN_ID, CMD_PROMOTE, args)
      // Refresh history so the row's promoted-state is visible to a
      // future enhancement (today the row stays — promote does not
      // delete the source). The saved-commands cache is reloaded by
      // the caller (it owns its own store).
      await get().loadHistory(api)
      return saved.slug
    } catch (err) {
      set({ error: String(err) })
      throw err
    }
  },

  reset: () => set({ rows: [], loaded: false, loading: false, error: null }),
}))
