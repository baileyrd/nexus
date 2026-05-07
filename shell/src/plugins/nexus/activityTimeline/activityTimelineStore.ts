import { create } from 'zustand'

/**
 * BL-037 / BL-052 — universal activity timeline store.
 *
 * Mirrors the (BL-037-only) on-disk JSONL log at
 * `<forge>/.forge/ai-activity.log` plus the (BL-052-only) live bus
 * stream from non-AI emitters (storage / git / terminal / workflow).
 * Hydrated once on activate via `com.nexus.ai::activity_list` (which
 * still owns the persisted AI history) and kept in sync via the
 * universal `com.nexus.activity.appended` bus topic plus the legacy
 * `com.nexus.ai.activity_appended` topic for back-compat.
 *
 * Wire shape mirrors `ActivityEntry` in
 * `crates/nexus-types/src/activity.rs`. We hand-define the type here
 * so the store stays compileable when the AI plugin is disabled — the
 * IPC handler returns an empty array in that case.
 */

/**
 * Surface tag — was AI-only pre-BL-052; now covers every emitter.
 * The wire is a string so a future emitter (community plugin) doesn't
 * crash deserialisation; we widen the union to the values we know how
 * to render and treat anything else as `'other'` at render time.
 */
export type ActivitySurface =
  | 'chat'
  | 'ask'
  | 'cmdi'
  | 'ghost'
  | 'complete'
  | 'enrich'
  | 'file'
  | 'process'
  | 'git'
  | 'workflow'
  | 'capability'
  | 'other'

export type ActivityOutcome = 'ok' | 'error' | 'cancelled'

export interface ActivityToolCall {
  name: string
  ok: boolean
}

export interface ActivityEntry {
  id: string
  timestamp: string
  session_id: string
  surface: ActivitySurface
  /**
   * BL-052 origin discriminator — wire form is one of:
   * `ai` / `user` / `git` / `storage` / `capability` /
   * `plugin:<id>` / `workflow:<id>` / `agent:<session>` /
   * `terminal:<session>`. Defaults to `'ai'` for legacy entries.
   */
  origin: string
  provider?: string | null
  model?: string | null
  prompt: string
  files?: string[]
  tool_calls?: ActivityToolCall[]
  outcome: ActivityOutcome
  error?: string | null
  duration_ms?: number | null
}

/**
 * BL-052 — coarse origin kind extracted from {@link ActivityEntry.origin}.
 * Used by the origin filter chip; entries with custom plugin / agent /
 * workflow ids collapse to their kind ("plugin", "agent", "workflow")
 * so the filter stays a small enum.
 */
export type ActivityOriginKind =
  | 'ai'
  | 'user'
  | 'plugin'
  | 'workflow'
  | 'agent'
  | 'terminal'
  | 'git'
  | 'storage'
  | 'capability'

/**
 * Split a wire `origin` string into its kind. `terminal:abc-123` →
 * `'terminal'`; `ai` → `'ai'`. Unknown shapes degrade to `'plugin'`
 * to match the Rust-side {@link ActivityOrigin::from_wire} fallback.
 */
export function originKind(origin: string): ActivityOriginKind {
  const colonAt = origin.indexOf(':')
  const head = colonAt < 0 ? origin : origin.slice(0, colonAt)
  switch (head) {
    case 'ai':
    case 'user':
    case 'plugin':
    case 'workflow':
    case 'agent':
    case 'terminal':
    case 'git':
    case 'storage':
    case 'capability':
      return head
    default:
      return 'plugin'
  }
}

/** Cap on entries kept in the store. The on-disk log is also bounded
 *  (~1k entries by default); this is a defence-in-depth so an
 *  oversized JSONL file doesn't blow the renderer up. */
export const TIMELINE_ENTRIES_CAP = 1000

/** ISO-8601 calendar date (`YYYY-MM-DD`) as accepted by `<input type="date">`.
 *  Compared against the entry timestamp's local-date prefix. */
export type IsoDate = string

interface ActivityTimelineState {
  /** Newest entry first. */
  entries: ActivityEntry[]
  /** Free-text filter. Matches surface, provider, model, prompt, files. */
  filter: string
  /** Surface filter. `null` = all surfaces. */
  surfaceFilter: ActivitySurface | null
  /** BL-052 origin filter. `null` = all origins. */
  originFilter: ActivityOriginKind | null
  /** Session filter. `null` = all sessions. */
  sessionFilter: string | null
  /** Inclusive date-range filter (`YYYY-MM-DD`); either bound may be
   *  null, meaning unbounded on that side. */
  dateFrom: IsoDate | null
  dateTo: IsoDate | null
  /** Has the initial hydrate completed? Used to render an empty state
   *  vs a loading state when the user opens the pane. */
  hydrated: boolean

  hydrate(entries: ActivityEntry[]): void
  prepend(entry: ActivityEntry): void
  setFilter(filter: string): void
  setSurfaceFilter(s: ActivitySurface | null): void
  setOriginFilter(o: ActivityOriginKind | null): void
  setSessionFilter(id: string | null): void
  setDateRange(from: IsoDate | null, to: IsoDate | null): void
  resetFilters(): void
  clear(): void
}

export const useActivityTimelineStore = create<ActivityTimelineState>(
  (set) => ({
    entries: [],
    filter: '',
    surfaceFilter: null,
    originFilter: null,
    sessionFilter: null,
    dateFrom: null,
    dateTo: null,
    hydrated: false,

    hydrate: (entries) =>
      set({
        entries: entries.slice(0, TIMELINE_ENTRIES_CAP),
        hydrated: true,
      }),

    prepend: (entry) =>
      set((s) => {
        // BL-052 — emitters publish to both topics during the
        // back-compat window; the AI recorder fires both
        // `com.nexus.ai.activity_appended` (legacy) and
        // `com.nexus.activity.appended` (universal). Two prepend
        // calls with the same `id` would render twice. Dedupe by
        // checking the head of the list — entries arrive in causal
        // order so the duplicate, if present, is at index 0.
        if (s.entries.length > 0 && s.entries[0].id === entry.id) {
          return s
        }
        // Drop the trailing entry when we hit the cap so the most
        // recent activity always lands at the top.
        const next =
          s.entries.length >= TIMELINE_ENTRIES_CAP
            ? [entry, ...s.entries.slice(0, TIMELINE_ENTRIES_CAP - 1)]
            : [entry, ...s.entries]
        return { entries: next }
      }),

    setFilter: (filter) => set({ filter }),
    setSurfaceFilter: (surfaceFilter) => set({ surfaceFilter }),
    setOriginFilter: (originFilter) => set({ originFilter }),
    setSessionFilter: (sessionFilter) => set({ sessionFilter }),
    setDateRange: (dateFrom, dateTo) => set({ dateFrom, dateTo }),
    resetFilters: () =>
      set({
        filter: '',
        surfaceFilter: null,
        originFilter: null,
        sessionFilter: null,
        dateFrom: null,
        dateTo: null,
      }),
    clear: () => set({ entries: [] }),
  }),
)
