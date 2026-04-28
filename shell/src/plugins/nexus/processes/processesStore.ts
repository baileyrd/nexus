import { create } from 'zustand'

/**
 * State for the `nexus.processes` pane-mode view — a read-only
 * observatory of what's loaded in the kernel + shell and what's
 * flying across the event bus.
 *
 * Three axes:
 *   - `plugins`  — shell-side manifests (built-in + community) merged
 *                  at activate. Kernel-side plugin status is not
 *                  surfaced yet because `nexus-plugin-api` doesn't
 *                  expose a status IPC; folded in once it does.
 *   - `sessions` — runtime sessions with a lifecycle separate from
 *                  plugin load: terminal PTYs, MCP connections.
 *   - `events`   — rolling tail from `com.nexus.*` topic subscriptions.
 *                  Capped at PROCESS_EVENTS_CAP so the ring stays bounded on a
 *                  chatty workspace.
 */

export interface PluginItem {
  id: string
  name: string
  version: string
  source: 'builtin' | 'community' | 'kernel'
  /** Free-form state label from the source — 'active' / 'error' / 'inactive' / 'disabled' / ... */
  state: string
  error?: string
}

export interface SessionItem {
  id: string
  kind: 'terminal' | 'mcp'
  label: string
  detail?: string
}

export interface EventLine {
  timestampMs: number
  topic: string
  /** Pretty-stringified JSON — computed once at append so the view doesn't re-stringify on every render. */
  payloadJson: string
}

/** Hard cap on the event ring buffer. Oldest entries drop as new arrive. */
export const PROCESS_EVENTS_CAP = 500

interface ProcessesState {
  plugins: PluginItem[]
  sessions: SessionItem[]
  events: EventLine[]
  /** Which left-column category is expanded / which detail the right panel reflects. */
  selectedCategory: 'plugins' | 'sessions' | null
  /** Free-text event filter. Matches topic OR payload substring, case-insensitive. */
  filter: string
  /** Auto-scroll the event log to the newest line as events arrive. */
  follow: boolean

  setPlugins(ps: PluginItem[]): void
  setSessions(ss: SessionItem[]): void
  appendEvent(e: EventLine): void
  setSelectedCategory(c: 'plugins' | 'sessions' | null): void
  setFilter(f: string): void
  setFollow(f: boolean): void
  clearEvents(): void
}

export const useProcessesStore = create<ProcessesState>((set) => ({
  plugins: [],
  sessions: [],
  events: [],
  selectedCategory: 'plugins',
  filter: '',
  follow: true,

  setPlugins: (ps) => set({ plugins: ps }),
  setSessions: (ss) => set({ sessions: ss }),

  appendEvent: (e) =>
    set((s) => {
      // Drop the oldest entry when at cap. A single-pass slice keeps
      // this O(PROCESS_EVENTS_CAP) and avoids the allocation churn of an
      // Array.prototype.shift() on every append.
      const next = s.events.length >= PROCESS_EVENTS_CAP
        ? [...s.events.slice(s.events.length - PROCESS_EVENTS_CAP + 1), e]
        : [...s.events, e]
      return { events: next }
    }),

  setSelectedCategory: (c) => set({ selectedCategory: c }),
  setFilter: (f) => set({ filter: f }),
  setFollow: (f) => set({ follow: f }),
  clearEvents: () => set({ events: [] }),
}))
