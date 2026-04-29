// shell/src/plugins/nexus/enrich/enrichStore.ts
//
// BL-045 — transient state for the auto-enrichment accept-gate.
//
// The runtime listens to `files:saved`, throttles per-file, calls
// `com.nexus.ai::enrich_file`, and then pushes the resulting
// `EnrichmentProposal` into this store. The accept-gate UI subscribes
// to `pending` and renders an inline panel with Accept / Dismiss
// buttons. On Accept, the runtime issues
// `com.nexus.ai::enrich_apply` and clears the entry for that file.
//
// FU-5: pending proposals are keyed by `file_path` rather than a
// single slot. A second save of the same file overwrites its own
// proposal (latest wins, same as before); saves of *different*
// files queue alongside it instead of clobbering. The UI shows one
// proposal at a time (insertion order — first one in is the head)
// and exposes a "Dismiss all" affordance when the queue is non-empty.

import { create } from 'zustand'

/** Mirrors `nexus_ai::enrichment::EnrichmentProposal` (Rust). */
export interface EnrichmentProposal {
  path: string
  body_hash: string
  tags: string[]
  summary: string
  related: string[]
}

export interface EnrichState {
  /** Pending proposals, keyed by `file_path`. Insertion order is the
   *  display order — the head of the iterator is the active gate.
   *  A `Map` rather than an array so a same-file resave is an O(1)
   *  overwrite without searching. */
  pending: Map<string, EnrichmentProposal>
  /** True while `enrich_apply` is in flight for the head proposal. */
  applying: boolean
  /** Last error, surfaced in the panel. Cleared on next propose-success. */
  error: string | null

  setPending: (p: EnrichmentProposal) => void
  setApplying: (a: boolean) => void
  setError: (e: string | null) => void
  /** Drop the proposal for `path` (no-op if absent). */
  dismiss: (path: string) => void
  /** Drop every queued proposal in one shot. */
  dismissAll: () => void
}

export const useEnrichStore = create<EnrichState>((set) => ({
  pending: new Map<string, EnrichmentProposal>(),
  applying: false,
  error: null,
  setPending: (p) =>
    set((state) => {
      const next = new Map(state.pending)
      next.set(p.path, p)
      return { pending: next, error: null }
    }),
  setApplying: (a) => set({ applying: a }),
  setError: (e) => set({ error: e }),
  dismiss: (path) =>
    set((state) => {
      if (!state.pending.has(path)) return {}
      const next = new Map(state.pending)
      next.delete(path)
      return { pending: next, applying: false, error: null }
    }),
  dismissAll: () => set({ pending: new Map(), applying: false, error: null }),
}))

/** Helper: read the head (oldest) pending proposal. UI components
 *  treat this as "the currently visible proposal". */
export function headPending(
  state: Pick<EnrichState, 'pending'>,
): EnrichmentProposal | null {
  const it = state.pending.values().next()
  return it.done ? null : it.value
}
