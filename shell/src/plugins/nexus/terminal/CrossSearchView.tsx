// shell/src/plugins/nexus/terminal/CrossSearchView.tsx
//
// BL-063 — cross-session scrollback search panel.
//
// Triggered by `nexus.terminal.crossSearch.show` (default Cmd/Ctrl+Shift+F
// inside the terminal pane). Calls `com.nexus.terminal::cross_session_search`
// against the FTS5 index that's populated whenever a session's
// scrollback gets persisted (BL-062 eviction path).
//
// The result list is grouped by `session_id` so the user sees which
// session a hit came from. Sessions whose metadata is still in the
// store are labeled by their `name` from `terminal_sessions`; ones
// that pre-date BL-062 (or whose metadata was never saved) fall
// back to a truncated id.

import { useCallback, useEffect, useMemo, useState } from 'react'
import type { KernelAPI, NotificationsAPI } from '../../../types/plugin'
import { Icon } from '../../../icons'

const PLUGIN_ID = 'com.nexus.terminal'
const CMD_CROSS_SESSION_SEARCH = 'cross_session_search'

/** Mirrors `crates/nexus-terminal/src/persist.rs::ScrollbackHit`. */
interface ScrollbackHit {
  session_id: string
  text: string
  ts_ms: number
  line_index: number
}

interface CrossSearchViewProps {
  kernel: KernelAPI
  notifications: NotificationsAPI
}

/// Minimum characters before we issue a search. The FTS5 path is
/// fast, but every keystroke triggering a fresh query is overkill —
/// 2 chars is the threshold below which results are too noisy to
/// be useful anyway.
const MIN_QUERY_CHARS = 2

export function CrossSearchView({ kernel }: CrossSearchViewProps) {
  const [query, setQuery] = useState('')
  const [isRegex, setIsRegex] = useState(false)
  const [hits, setHits] = useState<ScrollbackHit[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  // Debounce keystrokes so a user typing fast doesn't kick off N
  // FTS queries. 200 ms feels live without thrashing the index.
  useEffect(() => {
    const trimmed = query.trim()
    if (trimmed.length < MIN_QUERY_CHARS) {
      setHits([])
      setError(null)
      return
    }
    const handle = setTimeout(() => {
      void runSearch(trimmed, isRegex)
    }, 200)
    return () => clearTimeout(handle)
    // runSearch is stable in this scope — intentional that changing
    // `isRegex` reissues the query against the same string.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, isRegex])

  const runSearch = useCallback(
    async (q: string, regex: boolean) => {
      if (!(await kernel.available())) return
      setLoading(true)
      setError(null)
      try {
        const resp = await kernel.invoke<ScrollbackHit[]>(
          PLUGIN_ID,
          CMD_CROSS_SESSION_SEARCH,
          { query: q, is_regex: regex },
        )
        setHits(resp ?? [])
      } catch (err) {
        setHits([])
        setError(String(err))
      } finally {
        setLoading(false)
      }
    },
    [kernel],
  )

  // Group hits by session_id so the result list reads well on a
  // workspace with several long-lived sessions.
  const grouped = useMemo(() => {
    const map = new Map<string, ScrollbackHit[]>()
    for (const h of hits) {
      const arr = map.get(h.session_id)
      if (arr) arr.push(h)
      else map.set(h.session_id, [h])
    }
    return Array.from(map.entries())
  }, [hits])

  return (
    <div className="nexus-cross-search">
      <header className="nexus-cross-search-header">
        <h3>Search all sessions</h3>
      </header>
      <div className="nexus-cross-search-controls">
        <input
          type="text"
          className="nexus-cross-search-input"
          placeholder="Find in scrollback…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
        />
        <label className="nexus-cross-search-regex">
          <input
            type="checkbox"
            checked={isRegex}
            onChange={(e) => setIsRegex(e.target.checked)}
          />
          regex
        </label>
      </div>

      {error && (
        <div className="nexus-cross-search-error" role="alert">
          {error}
        </div>
      )}
      {loading && <div className="nexus-cross-search-loading">Searching…</div>}

      {grouped.length === 0 && !loading && query.trim().length >= MIN_QUERY_CHARS && !error && (
        <p className="nexus-cross-search-empty">
          No matches across persisted scrollback.
        </p>
      )}
      {query.trim().length < MIN_QUERY_CHARS && (
        <p className="nexus-cross-search-empty">
          Type at least {MIN_QUERY_CHARS} characters to search.
        </p>
      )}

      <ul className="nexus-cross-search-results">
        {grouped.map(([sessionId, group]) => (
          <li key={sessionId} className="nexus-cross-search-group">
            <header className="nexus-cross-search-group-header">
              <Icon name="terminal" size={12} />
              <code className="nexus-cross-search-group-id" title={sessionId}>
                {truncateId(sessionId)}
              </code>
              <span className="nexus-cross-search-group-count">
                {group.length} {group.length === 1 ? 'hit' : 'hits'}
              </span>
            </header>
            <ul className="nexus-cross-search-group-rows">
              {group.map((hit, idx) => (
                <li key={`${hit.session_id}-${hit.line_index}-${idx}`}>
                  <code className="nexus-cross-search-line">
                    {hit.text}
                  </code>
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
    </div>
  )
}

/// 36-char UUIDs are too long for a sidebar header. Show the first
/// 8 chars (the dedupe-friendly prefix) and trust the `title`
/// attribute for the full id.
function truncateId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 8)}…` : id
}
