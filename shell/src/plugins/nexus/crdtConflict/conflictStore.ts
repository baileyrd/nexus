// BL-074 — resolver modal store.
//
// Each `com.nexus.editor.crdt.conflict.<relpath>` envelope becomes one
// modal request. The user resolves each row (Keep local / Use remote
// / Open file) independently; the request stays open until they
// dismiss or every row is resolved. Multiple envelopes (different
// files, or repeat events on the same file) queue behind the current
// request — same queue+current pattern as `pickStore`/`confirmStore`.

import { create } from 'zustand'

import type { ConflictDetail } from './types'

/** Per-row resolution choice. Drives the icon / disabled state on the
 *  matching button row in the modal. */
export type Resolution = 'pending' | 'kept_local' | 'used_remote' | 'skipped'

export interface ConflictRow {
  /** Wire payload for this conflict. */
  detail: ConflictDetail
  /** User's choice for this row (or 'pending' until they pick). */
  resolution: Resolution
  /** Last error message produced while applying this row's resolution.
   *  Surfaces inline so the user knows the click didn't take. */
  error: string | null
}

export interface ConflictRequest {
  /** Monotonic id assigned at enqueue time. Used to keyframe
   *  `useEffect`s in the modal. */
  id: number
  /** Forge-relative path of the file this conflict bundle is for. */
  relpath: string
  /** Per-conflict rows in the order Rust returned them. */
  rows: ConflictRow[]
}

interface ConflictStoreState {
  current: ConflictRequest | null
  queue: ConflictRequest[]
  /** Enqueue a fresh envelope for the given relpath. */
  enqueue(relpath: string, conflicts: ConflictDetail[]): void
  /** Update the resolution for a row in the current request. */
  setResolution(rowIdx: number, resolution: Resolution, error?: string | null): void
  /** Dismiss the current request (regardless of remaining 'pending'
   *  rows) and advance to the next queued one. */
  dismissCurrent(): void
}

let nextId = 1

export const useConflictStore = create<ConflictStoreState>((set, get) => ({
  current: null,
  queue: [],

  enqueue: (relpath, conflicts) => {
    if (conflicts.length === 0) return
    const req: ConflictRequest = {
      id: nextId++,
      relpath,
      rows: conflicts.map((detail) => ({
        detail,
        resolution: 'pending',
        error: null,
      })),
    }
    const s = get()
    if (s.current === null) {
      set({ current: req })
    } else {
      set({ queue: [...s.queue, req] })
    }
  },

  setResolution: (rowIdx, resolution, error = null) => {
    const s = get()
    if (!s.current) return
    if (rowIdx < 0 || rowIdx >= s.current.rows.length) return
    const updated = s.current.rows.map((row, i) =>
      i === rowIdx ? { ...row, resolution, error } : row,
    )
    set({ current: { ...s.current, rows: updated } })
  },

  dismissCurrent: () => {
    const s = get()
    if (!s.current) return
    const [next, ...rest] = s.queue
    set({ current: next ?? null, queue: rest })
  },
}))

/** Test-only helper. */
export function _resetConflictStoreForTests(): void {
  useConflictStore.setState({ current: null, queue: [] })
}
