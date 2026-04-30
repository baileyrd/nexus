import { create } from 'zustand'

/**
 * BL-037 — AI activity timeline store.
 *
 * Mirrors the on-disk JSONL log at `<forge>/.forge/ai-activity.log`
 * inside a Zustand store. Hydrated once on activate via
 * `com.nexus.ai::activity_list`, then kept in sync via the
 * `com.nexus.ai.activity_appended` bus topic published by the kernel
 * recorder after every successful append.
 *
 * Wire shape mirrors `ActivityEntry` in
 * `crates/nexus-ai/src/activity_log.rs`. We hand-define the type here
 * (rather than importing the ts-rs binding) so the store stays
 * compileable when the AI plugin is disabled — the IPC handler
 * returns an empty array in that case.
 */

export type ActivitySurface =
  | 'chat'
  | 'ask'
  | 'cmdi'
  | 'ghost'
  | 'complete'
  | 'enrich'
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
  provider?: string | null
  model?: string | null
  prompt: string
  files?: string[]
  tool_calls?: ActivityToolCall[]
  outcome: ActivityOutcome
  error?: string | null
  duration_ms?: number | null
}

/** Cap on entries kept in the store. The on-disk log is also bounded
 *  (~1k entries by default); this is a defence-in-depth so an
 *  oversized JSONL file doesn't blow the renderer up. */
export const TIMELINE_ENTRIES_CAP = 1000

interface ActivityTimelineState {
  /** Newest entry first. */
  entries: ActivityEntry[]
  /** Free-text filter. Matches surface, provider, model, prompt, files. */
  filter: string
  /** Surface filter. `null` = all surfaces. */
  surfaceFilter: ActivitySurface | null
  /** Has the initial hydrate completed? Used to render an empty state
   *  vs a loading state when the user opens the pane. */
  hydrated: boolean

  hydrate(entries: ActivityEntry[]): void
  prepend(entry: ActivityEntry): void
  setFilter(filter: string): void
  setSurfaceFilter(s: ActivitySurface | null): void
  clear(): void
}

export const useActivityTimelineStore = create<ActivityTimelineState>(
  (set) => ({
    entries: [],
    filter: '',
    surfaceFilter: null,
    hydrated: false,

    hydrate: (entries) =>
      set({
        entries: entries.slice(0, TIMELINE_ENTRIES_CAP),
        hydrated: true,
      }),

    prepend: (entry) =>
      set((s) => {
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
    clear: () => set({ entries: [] }),
  }),
)
