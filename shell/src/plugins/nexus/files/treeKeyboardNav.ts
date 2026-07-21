// C73 (#426) — pure index arithmetic for arrow-key tree navigation.
// Operates on the same flattened, depth-annotated row array the
// virtualizer already renders from (`flattenTree.ts`), so "next/previous
// visible node" is just adjacent-index movement and "parent"/"first
// child" fall out of comparing `depth`. Kept separate from FilesTree.tsx
// so the navigation math is unit-testable without React or a DOM.
import type { FlatRow } from './flattenTree'

/** Index of the next visible row below `fromIdx`, clamped to the last row. */
export function nextVisibleIndex(rows: FlatRow[], fromIdx: number): number {
  if (rows.length === 0) return fromIdx
  return Math.min(fromIdx + 1, rows.length - 1)
}

/** Index of the next visible row above `fromIdx`, clamped to the first row. */
export function prevVisibleIndex(rows: FlatRow[], fromIdx: number): number {
  if (rows.length === 0) return fromIdx
  return Math.max(fromIdx - 1, 0)
}

/** Index of `fromIdx`'s nearest ancestor row (the first row above it at a
 *  shallower depth), or `fromIdx` itself when already at the root depth
 *  (no parent to move to). */
export function parentIndex(rows: FlatRow[], fromIdx: number): number {
  const depth = rows[fromIdx]?.depth
  if (depth === undefined || depth === 0) return fromIdx
  for (let i = fromIdx - 1; i >= 0; i--) {
    if (rows[i].depth < depth) return i
  }
  return fromIdx
}

/** Index of `fromIdx`'s first child row (depth + 1, immediately
 *  following in the flattened array), or `null` when `fromIdx` has no
 *  expanded children right now (collapsed, childless, or not a
 *  directory — the caller is expected to check that separately). */
export function firstChildIndex(rows: FlatRow[], fromIdx: number): number | null {
  const depth = rows[fromIdx]?.depth
  if (depth === undefined) return null
  const next = rows[fromIdx + 1]
  if (next && next.depth === depth + 1) return fromIdx + 1
  return null
}
