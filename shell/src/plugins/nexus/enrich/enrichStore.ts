// shell/src/plugins/nexus/enrich/enrichStore.ts
//
// BL-045 — transient state for the auto-enrichment accept-gate.
//
// The runtime listens to `files:saved`, throttles per-file, calls
// `com.nexus.ai::enrich_file`, and then pushes the resulting
// `EnrichmentProposal` into this store. The accept-gate UI subscribes
// to `pending` and renders an inline panel with Accept / Dismiss
// buttons. On Accept, the runtime issues
// `com.nexus.ai::enrich_apply` and clears the entry.

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
  /** Most recent proposal awaiting user accept/dismiss, if any. v1
   *  shows one at a time (per-file). The runtime overwrites the slot
   *  whenever a fresher proposal arrives — the user always sees the
   *  latest suggestion for the most recently saved file. */
  pending: EnrichmentProposal | null
  /** True while `enrich_apply` is in flight. UI greys out the buttons. */
  applying: boolean
  /** Last error, surfaced in the panel. Cleared on next propose-success. */
  error: string | null

  setPending: (p: EnrichmentProposal | null) => void
  setApplying: (a: boolean) => void
  setError: (e: string | null) => void
  dismiss: () => void
}

export const useEnrichStore = create<EnrichState>((set) => ({
  pending: null,
  applying: false,
  error: null,
  setPending: (p) => set({ pending: p, error: null }),
  setApplying: (a) => set({ applying: a }),
  setError: (e) => set({ error: e }),
  dismiss: () => set({ pending: null, applying: false, error: null }),
}))
