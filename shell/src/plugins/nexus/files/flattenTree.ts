import type { FilesDirEntry, SortMode } from './filesStore'

/** Directory extensions that behave like documents — never expand
 *  their contents in the tree. Mirrors the BUNDLE_DIR_EXTS list in
 *  FilesTree; kept here too so flattening doesn't depend on the React
 *  module. */
const BUNDLE_DIR_EXTS = new Set(['bases'])

export function isBundleDir(entry: FilesDirEntry): boolean {
  if (!entry.isDir) return false
  const dot = entry.name.lastIndexOf('.')
  if (dot < 0) return false
  return BUNDLE_DIR_EXTS.has(entry.name.slice(dot + 1).toLowerCase())
}

export interface FlatRow {
  entry: FilesDirEntry
  depth: number
}

/** Numeric comparator that treats `undefined` as "worst" (pushed to
 *  the end) and breaks ties by case-insensitive name. */
function compareNullableNumber(
  a: number | undefined,
  b: number | undefined,
  nameA: string,
  nameB: string,
): number {
  if (a === undefined && b === undefined) {
    return nameA.toLowerCase().localeCompare(nameB.toLowerCase())
  }
  if (a === undefined) return 1
  if (b === undefined) return -1
  if (a !== b) return a - b
  return nameA.toLowerCase().localeCompare(nameB.toLowerCase())
}

export function sortEntries(entries: FilesDirEntry[], mode: SortMode): FilesDirEntry[] {
  const sorted = [...entries]
  sorted.sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
    switch (mode) {
      case 'nameAsc':
        return a.name.toLowerCase().localeCompare(b.name.toLowerCase())
      case 'nameDesc':
        return b.name.toLowerCase().localeCompare(a.name.toLowerCase())
      case 'modifiedDesc':
        return compareNullableNumber(b.modifiedMs, a.modifiedMs, a.name, b.name)
      case 'modifiedAsc':
        return compareNullableNumber(a.modifiedMs, b.modifiedMs, a.name, b.name)
      case 'createdDesc':
        return compareNullableNumber(b.createdMs, a.createdMs, a.name, b.name)
      case 'createdAsc':
        return compareNullableNumber(a.createdMs, b.createdMs, a.name, b.name)
    }
  })
  return sorted
}

/**
 * Flatten the visible portion of the tree into an index-addressable
 * array. A directory contributes its entry plus, when expanded and its
 * children are loaded, recursive walk of those children. Bundle dirs
 * (e.g. `.bases`) never expand even when present in `expanded`.
 *
 * Pure: same inputs always produce the same output. Used by the
 * virtualizer to mount only the rows in view.
 */
export function flattenTree(
  rootEntries: FilesDirEntry[],
  children: Record<string, FilesDirEntry[]>,
  expanded: Set<string>,
  sortMode: SortMode,
): FlatRow[] {
  const out: FlatRow[] = []
  const walk = (entries: FilesDirEntry[], depth: number): void => {
    for (const entry of sortEntries(entries, sortMode)) {
      out.push({ entry, depth })
      if (!entry.isDir || isBundleDir(entry)) continue
      if (!expanded.has(entry.relpath)) continue
      const kids = children[entry.relpath]
      if (!kids) continue
      walk(kids, depth + 1)
    }
  }
  walk(rootEntries, 0)
  return out
}
