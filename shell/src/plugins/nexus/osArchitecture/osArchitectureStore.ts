// BL-054 Phase 2 — store for the osArchitecture panel.
//
// Carries (1) the parsed architecture.md, (2) the drift report against
// live skill / workflow registries, and (3) a per-domain collapse map
// so the panel state survives a re-render. Refresh is owned by the
// plugin's index.ts (kernel + storage IPC); the store is a passive
// projection.

import { create } from 'zustand'

import type { Architecture } from './architectureParser'
import type { DriftReport } from './driftDetect'

export type LoadStatus = 'idle' | 'loading' | 'ok' | 'missing' | 'error'

interface OsArchitectureState {
  status: LoadStatus
  /** Set when status === 'error'. */
  error: string | null
  /** Set whenever a parse has completed (status === 'ok' or 'missing'). */
  architecture: Architecture | null
  /** Drift report, recomputed in the same pass as `architecture`. */
  drift: DriftReport | null
  /** Per-domain collapse — true means the domain's task list is hidden. */
  collapsed: Set<string>

  setStatus(status: LoadStatus, error?: string | null): void
  setData(architecture: Architecture, drift: DriftReport): void
  setMissing(): void
  toggleCollapsed(domain: string): void
  reset(): void
}

const INITIAL: Pick<OsArchitectureState, 'status' | 'error' | 'architecture' | 'drift' | 'collapsed'> = {
  status: 'idle',
  error: null,
  architecture: null,
  drift: null,
  collapsed: new Set<string>(),
}

export const useOsArchitectureStore = create<OsArchitectureState>((set) => ({
  ...INITIAL,

  setStatus: (status, error = null) =>
    set({ status, error }),

  setData: (architecture, drift) =>
    set({ status: 'ok', error: null, architecture, drift }),

  setMissing: () =>
    set({
      status: 'missing',
      error: null,
      architecture: { preamble: '', domains: [] },
      drift: { byTask: new Map(), unattached: [] },
    }),

  toggleCollapsed: (domain) =>
    set((s) => {
      const next = new Set(s.collapsed)
      if (next.has(domain)) next.delete(domain)
      else next.add(domain)
      return { collapsed: next }
    }),

  reset: () => set({ ...INITIAL, collapsed: new Set<string>() }),
}))
