/**
 * View state for `nexus.sessions` (RFC 0008, Phase 5.4).
 *
 * Single source of truth for the session-tree panel: the runtime mutates it,
 * the React view reads it, and tests poke it directly. No persistence — the
 * forest + transcript + checkpoints are fetched on demand and held only while
 * the panel is open. Transcript decoding is reused from the `nexus.agent`
 * plugin (same `session_get` shape).
 */

import { create } from 'zustand'

import type { SessionTranscript } from '../agent/sessionStore'
import type { SessionNode } from './sessionTree'

/** A named `(session, round)` bookmark — decoded from `session_checkpoints`. */
export interface Checkpoint {
  name: string
  sessionId: string
  round: number
  createdAt: string
}

export function decodeCheckpoints(raw: unknown): Checkpoint[] {
  if (!Array.isArray(raw)) return []
  const out: Checkpoint[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const name = typeof r.name === 'string' ? r.name : null
    const sessionId = typeof r.session_id === 'string' ? r.session_id : null
    if (!name || !sessionId) continue
    out.push({
      name,
      sessionId,
      round: typeof r.round === 'number' ? r.round : 0,
      createdAt: typeof r.created_at === 'string' ? r.created_at : '',
    })
  }
  return out
}

export interface SessionsState {
  // ── Forest ────────────────────────────────────────────────────────────
  nodes: SessionNode[]
  loading: boolean
  error: string | null

  // ── Selection ─────────────────────────────────────────────────────────
  selectedId: string | null
  transcript: SessionTranscript | null
  transcriptError: string | null

  // ── Checkpoints ───────────────────────────────────────────────────────
  checkpoints: Checkpoint[]

  /** A fork verb (resume/branch/rewind) is in flight — disables the actions. */
  busy: boolean

  // ── Mutators ──────────────────────────────────────────────────────────
  setNodes(nodes: SessionNode[]): void
  setLoading(loading: boolean): void
  setError(error: string | null): void
  setSelected(
    id: string | null,
    transcript: SessionTranscript | null,
    error: string | null,
  ): void
  setCheckpoints(checkpoints: Checkpoint[]): void
  setBusy(busy: boolean): void
  reset(): void
}

type Mutators = {
  [K in keyof SessionsState as SessionsState[K] extends (...a: never[]) => unknown
    ? K
    : never]: SessionsState[K]
}

const INITIAL: Omit<SessionsState, keyof Mutators> = {
  nodes: [],
  loading: false,
  error: null,
  selectedId: null,
  transcript: null,
  transcriptError: null,
  checkpoints: [],
  busy: false,
}

export const useSessionsStore = create<SessionsState>((set) => ({
  ...INITIAL,
  setNodes: (nodes) => set({ nodes }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  setSelected: (selectedId, transcript, transcriptError) =>
    set({ selectedId, transcript, transcriptError }),
  setCheckpoints: (checkpoints) => set({ checkpoints }),
  setBusy: (busy) => set({ busy }),
  reset: () => set({ ...INITIAL }),
}))
