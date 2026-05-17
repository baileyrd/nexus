// BL-129 follow-up — Dream Cycle inbox store.
//
// Mirrors `com.nexus.storage::list_draft_relations` rows in memory so
// the inbox view can render them and the plugin can mutate the list
// optimistically on approve / skip without re-fetching every action.

import { create } from 'zustand'

export interface DraftRelationRow {
  from: string
  target: string
  /** Relation kind. Carries the on-disk `type:` value (canonical). */
  type: string
  confidence: number
  relpath: string
}

/** Stable client key for a row — the (from, target, type) triple is
 *  unique within an entity file. */
export function rowKey(row: { from: string; target: string; type: string }): string {
  return `${row.from}${row.target}${row.type}`
}

interface DreamCycleState {
  rows: DraftRelationRow[]
  total: number
  truncated: boolean
  hydrated: boolean
  /** Per-row work state — approve / skip kicks an entity_upsert which
   *  may take a moment. The view renders the buttons disabled while
   *  the row is `pending`. */
  pending: Set<string>
  hydrate(rows: DraftRelationRow[], total: number, truncated: boolean): void
  markPending(key: string): void
  clearPending(key: string): void
  removeRow(key: string): void
}

export const useDreamCycleStore = create<DreamCycleState>((set) => ({
  rows: [],
  total: 0,
  truncated: false,
  hydrated: false,
  pending: new Set(),
  hydrate(rows, total, truncated) {
    set({ rows, total, truncated, hydrated: true })
  },
  markPending(key) {
    set((s) => {
      const next = new Set(s.pending)
      next.add(key)
      return { pending: next }
    })
  },
  clearPending(key) {
    set((s) => {
      const next = new Set(s.pending)
      next.delete(key)
      return { pending: next }
    })
  },
  removeRow(key) {
    set((s) => {
      const rows = s.rows.filter((r) => rowKey(r) !== key)
      const dropped = s.rows.length - rows.length
      return {
        rows,
        total: Math.max(0, s.total - dropped),
        truncated: s.truncated && rows.length < s.total - dropped,
      }
    })
  },
}))
