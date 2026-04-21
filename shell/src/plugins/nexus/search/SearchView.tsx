import { useEffect, useRef } from 'react'
import { useSearchStore, type SearchHit } from './searchStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { registerFocuser, scheduleSearch } from './searchRuntime'

const EVENT_FILE_OPEN = 'files:open'

interface SearchViewProps {
  onHitActivate: (hit: SearchHit) => void
}

/**
 * Basename of a forge-relative path. `""` maps to `""`. Forward-slash
 * only — the storage plugin never emits backslashes.
 */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/**
 * Full-text search over the active workspace. Renders a search input
 * at the top of the sidebar and a result list beneath. Keyboard
 * navigation lives on the input — the result rows themselves are not
 * focusable (they stay visible while the user continues typing).
 */
export function SearchView({ onHitActivate }: SearchViewProps) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const query = useSearchStore((s) => s.query)
  const results = useSearchStore((s) => s.results)
  const loading = useSearchStore((s) => s.loading)
  const error = useSearchStore((s) => s.error)
  const selectedIndex = useSearchStore((s) => s.selectedIndex)
  const setQuery = useSearchStore((s) => s.setQuery)
  const setSelectedIndex = useSearchStore((s) => s.setSelectedIndex)

  const inputRef = useRef<HTMLInputElement | null>(null)
  const listRef = useRef<HTMLDivElement | null>(null)

  // Expose `focus()` to the runtime so the `nexus.search.focus`
  // command can reach it. registerFocuser also drains any pending
  // focus request queued while the view was unmounted — covers the
  // first time a user hits Ctrl+Shift+F before the sidebar has ever
  // shown the search view.
  useEffect(() => {
    const focus = () => {
      // requestAnimationFrame to let the sidedock finish mounting us
      // if `revealLeaf` was called in the same tick.
      requestAnimationFrame(() => inputRef.current?.focus())
    }
    registerFocuser(focus)
    // Autofocus on first mount (the standard flow: activity-bar
    // click → focus command → revealLeaf → SearchView mounts).
    focus()
    return () => registerFocuser(null)
  }, [])

  // Keep the selected row scrolled into view as arrow keys walk past
  // the visible window.
  useEffect(() => {
    const list = listRef.current
    if (!list) return
    const row = list.querySelector<HTMLDivElement>(
      `[data-row-idx="${selectedIndex}"]`,
    )
    row?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex, results])

  const onInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value
    setQuery(value)
    scheduleSearch(value)
  }

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      if (results.length > 0) {
        setSelectedIndex(Math.min(results.length - 1, selectedIndex + 1))
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      if (results.length > 0) {
        setSelectedIndex(Math.max(0, selectedIndex - 1))
      }
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const picked = results[selectedIndex] ?? results[0]
      if (picked) onHitActivate(picked)
    }
  }

  const inputEl = (
    <input
      ref={inputRef}
      type="search"
      value={query}
      onChange={onInputChange}
      onKeyDown={onInputKeyDown}
      placeholder="Search…"
      spellCheck={false}
      autoComplete="off"
      style={{
        background: 'transparent',
        border: 0,
        borderBottom: '1px solid var(--line-soft)',
        outline: 0,
        padding: '10px 14px',
        color: 'var(--fg)',
        fontFamily: 'var(--f-ui)',
        width: '100%',
        boxSizing: 'border-box',
      }}
    />
  )

  // Empty-state body. Precedence: no workspace > error > loading >
  // empty query > no matches > results.
  let body: React.ReactNode
  if (!rootPath) {
    body = <StateMessage color="var(--fg-dim)">Open a workspace to search.</StateMessage>
  } else if (error) {
    body = <StateMessage color="var(--risk)">{error}</StateMessage>
  } else if (loading) {
    body = <StateMessage color="var(--fg-muted)">Searching…</StateMessage>
  } else if (query.trim() === '') {
    body = <StateMessage color="var(--fg-dim)">Type to search.</StateMessage>
  } else if (results.length === 0) {
    body = <StateMessage color="var(--fg-dim)">No matches.</StateMessage>
  } else {
    body = (
      <div ref={listRef} style={{ overflowY: 'auto', flex: 1 }}>
        {results.map((hit, idx) => (
          <ResultRow
            key={hit.relpath}
            index={idx}
            hit={hit}
            selected={idx === selectedIndex}
            onHover={() => setSelectedIndex(idx)}
            onPick={() => onHitActivate(hit)}
          />
        ))}
      </div>
    )
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        width: '100%',
      }}
    >
      {inputEl}
      {body}
    </div>
  )
}

function StateMessage({
  children,
  color,
}: {
  children: React.ReactNode
  color: string
}) {
  return (
    <div
      style={{
        padding: '12px 14px',
        color,
        fontFamily: 'var(--f-ui)',
        fontSize: 12,
      }}
    >
      {children}
    </div>
  )
}

interface ResultRowProps {
  index: number
  hit: SearchHit
  selected: boolean
  onHover: () => void
  onPick: () => void
}

function ResultRow({ index, hit, selected, onHover, onPick }: ResultRowProps) {
  const name = basename(hit.relpath) || hit.relpath
  const showSnippet = hit.snippet && hit.snippet.trim().length > 0

  return (
    <div
      data-row-idx={index}
      role="button"
      tabIndex={-1}
      onMouseEnter={onHover}
      onClick={onPick}
      style={{
        padding: '8px 14px',
        cursor: 'pointer',
        background: selected ? 'var(--accent-soft)' : 'transparent',
        transition: 'background 0.06s',
        fontFamily: 'var(--f-ui)',
        borderBottom: '1px solid var(--line-soft)',
      }}
    >
      <div
        style={{
          color: 'var(--fg)',
          fontSize: 13,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {name}
      </div>
      <div
        style={{
          color: 'var(--fg-dim)',
          fontSize: 11,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          marginTop: 1,
        }}
      >
        {hit.relpath}
      </div>
      {showSnippet && (
        <div
          style={{
            color: 'var(--fg-muted)',
            fontSize: 12,
            marginTop: 4,
            display: '-webkit-box',
            WebkitLineClamp: 3,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
          }}
        >
          {hit.snippet}
        </div>
      )}
    </div>
  )
}

export { EVENT_FILE_OPEN }
