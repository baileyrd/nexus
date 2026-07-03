// C5 (#358) — pure decode + fuzzy-match logic for the file quick-switcher,
// kept separate from index.ts/QuickSwitcher.tsx so it's testable without
// mocking the plugin/modal stack (mirrors taskDashboard/taskGrouping.ts).
//
// The subsequence-scoring algorithm mirrors commandPalette/match.ts's
// filterCommands (last-matched-character-index, ties broken
// alphabetically) but is duplicated rather than shared: it operates on a
// forge-relative path instead of a CommandEntry, and reuse would mean
// bending an already-simple, already-tested module around a second
// unrelated shape.

/** Wire shape of a `com.nexus.storage::query_files` row (`FileRecord`). */
export interface FileEntry {
  path: string
  file_type: string
  modified_at: number
}

/** Coerce a `query_files` response (a bare JSON array of file rows). */
export function decodeFiles(raw: unknown): FileEntry[] {
  if (!Array.isArray(raw)) return []
  const out: FileEntry[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    if (typeof r.path !== 'string' || r.path === '') continue
    out.push({
      path: r.path,
      file_type: typeof r.file_type === 'string' ? r.file_type : 'markdown',
      modified_at: typeof r.modified_at === 'number' ? r.modified_at : 0,
    })
  }
  return out
}

/**
 * Subsequence match: every character of `query` (already lowercased) must
 * appear in `haystack` (already lowercased) in order, not necessarily
 * contiguous. Returns the index of the last matched character (lower is a
 * tighter/earlier match, used as the sort key) or `null` on no match.
 */
export function subsequenceScore(haystack: string, query: string): number | null {
  if (query.length === 0) return -1
  let qi = 0
  let lastMatchIdx = -1
  for (let hi = 0; hi < haystack.length && qi < query.length; hi++) {
    if (haystack[hi] === query[qi]) {
      lastMatchIdx = hi
      qi++
    }
  }
  return qi === query.length ? lastMatchIdx : null
}

export interface ScoredFile {
  entry: FileEntry
  score: number
}

/**
 * Filter + rank `files` against `query`. Empty query returns `recentPaths`
 * first (in MRU order — most-recent first), then every other file
 * alphabetically — the VS Code Ctrl+P "recents when empty" convention. A
 * non-empty query fuzzy-matches on path, best subsequence match first,
 * recency as a tiebreak among equal-quality matches, path alphabetical as
 * the final tiebreak.
 */
export function filterFiles(
  files: FileEntry[],
  query: string,
  recentPaths: string[],
): ScoredFile[] {
  const q = query.toLowerCase().trim()
  const recentRank = new Map(recentPaths.map((p, i) => [p, i]))

  if (q.length === 0) {
    const byPath = new Map(files.map((f) => [f.path, f]))
    const recents: FileEntry[] = []
    for (const p of recentPaths) {
      const f = byPath.get(p)
      if (f) recents.push(f)
    }
    const rest = files
      .filter((f) => !recentRank.has(f.path))
      .sort((a, b) => a.path.localeCompare(b.path))
    return [...recents, ...rest].map((entry) => ({ entry, score: 0 }))
  }

  const scored: ScoredFile[] = []
  for (const entry of files) {
    const idx = subsequenceScore(entry.path.toLowerCase(), q)
    if (idx !== null) scored.push({ entry, score: idx })
  }
  scored.sort((a, b) => {
    if (a.score !== b.score) return a.score - b.score
    const aRecent = recentRank.get(a.entry.path)
    const bRecent = recentRank.get(b.entry.path)
    if (aRecent !== undefined && bRecent !== undefined) return aRecent - bRecent
    if (aRecent !== undefined) return -1
    if (bRecent !== undefined) return 1
    return a.entry.path.localeCompare(b.entry.path)
  })
  return scored
}

/** `true` when `fileType` should be hidden unless "show attachments" is on. */
export function isAttachment(fileType: string): boolean {
  return fileType === 'attachment'
}

const MAX_RECENTS = 20

/**
 * Push `path` to the front of `recents`, de-duplicating and capping at
 * [`MAX_RECENTS`] — the MRU-list update the plugin persists via
 * `api.configuration.setValue` on every file open.
 */
export function pushRecent(recents: string[], path: string): string[] {
  return [path, ...recents.filter((p) => p !== path)].slice(0, MAX_RECENTS)
}
