// shell/src/plugins/nexus/searchPanel/SearchPanelView.tsx
//
// BL-078 — multi-file find / replace UI.
//
// Lives as a sidebar leaf. Top: query input + checkboxes + replace
// row (replace input + apply button). Body: result tree grouped by
// file with one expand caret per group. Click a hit row → emits
// `files:open` so the editor brings the file into focus.
//
// The `editor:scrollToHeading` event the outline uses lands the
// view at a heading — there's no public "scroll to line" yet, so
// click-to-jump opens the file but doesn't scroll to the matched
// line. That's a follow-up; the file will at least open.

import { useCallback, useMemo } from 'react'
import type { KernelAPI, EventsAPI } from '../../../types/plugin'
import { Icon } from '../../../icons'
import {
  useSearchPanelStore,
  type FileMatches,
  type LineMatch,
} from './searchPanelStore'

interface SearchPanelViewProps {
  kernel: KernelAPI
  events: EventsAPI
}

export function SearchPanelView({ kernel, events }: SearchPanelViewProps) {
  const query = useSearchPanelStore((s) => s.query)
  const replacement = useSearchPanelStore((s) => s.replacement)
  const isRegex = useSearchPanelStore((s) => s.isRegex)
  const caseSensitive = useSearchPanelStore((s) => s.caseSensitive)
  const wholeWord = useSearchPanelStore((s) => s.wholeWord)
  const results = useSearchPanelStore((s) => s.results)
  const searched = useSearchPanelStore((s) => s.searched)
  const loading = useSearchPanelStore((s) => s.loading)
  const replacing = useSearchPanelStore((s) => s.replacing)
  const error = useSearchPanelStore((s) => s.error)
  const expanded = useSearchPanelStore((s) => s.expanded)
  const lastReplace = useSearchPanelStore((s) => s.lastReplace)

  const setQuery = useSearchPanelStore((s) => s.setQuery)
  const setReplacement = useSearchPanelStore((s) => s.setReplacement)
  const setIsRegex = useSearchPanelStore((s) => s.setIsRegex)
  const setCaseSensitive = useSearchPanelStore((s) => s.setCaseSensitive)
  const setWholeWord = useSearchPanelStore((s) => s.setWholeWord)
  const toggleExpanded = useSearchPanelStore((s) => s.toggleExpanded)
  const runSearch = useSearchPanelStore((s) => s.runSearch)
  const applyReplace = useSearchPanelStore((s) => s.applyReplace)

  const totalHits = useMemo(
    () => results.reduce((acc, f) => acc + f.hits.length, 0),
    [results],
  )

  const submitSearch = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault()
      void runSearch(kernel)
    },
    [kernel, runSearch],
  )

  const openHit = useCallback(
    (relpath: string) => {
      // The files plugin's `files:open` listener handles routing
      // into a workspace leaf. Name is derived from the basename
      // for the tab title.
      const name =
        relpath
          .split(/[\\/]/)
          .filter((s) => s.length > 0)
          .pop() ?? relpath
      events.emit('files:open', { relpath, name })
    },
    [events],
  )

  const replaceAll = useCallback(() => {
    void applyReplace(kernel, null)
  }, [applyReplace, kernel])

  const replaceFile = useCallback(
    (relpath: string) => {
      void applyReplace(kernel, [relpath])
    },
    [applyReplace, kernel],
  )

  return (
    <div className="nexus-search-panel">
      <header className="nexus-search-panel-header">
        <h3>Search in files</h3>
      </header>

      <form className="nexus-search-panel-form" onSubmit={submitSearch}>
        <input
          type="text"
          className="nexus-search-panel-input"
          placeholder="Find in forge…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
        />
        <div className="nexus-search-panel-flags">
          <label title="Match case (Aa)">
            <input
              type="checkbox"
              checked={caseSensitive}
              onChange={(e) => setCaseSensitive(e.target.checked)}
            />
            Aa
          </label>
          <label title="Whole word (\\b)">
            <input
              type="checkbox"
              checked={wholeWord}
              onChange={(e) => setWholeWord(e.target.checked)}
            />
            ab
          </label>
          <label title="Regex">
            <input
              type="checkbox"
              checked={isRegex}
              onChange={(e) => setIsRegex(e.target.checked)}
            />
            .*
          </label>
          <button
            type="submit"
            className="nexus-search-panel-submit"
            disabled={loading || query.trim().length === 0}
          >
            {loading ? '…' : 'Find'}
          </button>
        </div>
        <input
          type="text"
          className="nexus-search-panel-input"
          placeholder="Replace with…"
          value={replacement}
          onChange={(e) => setReplacement(e.target.value)}
        />
        <div className="nexus-search-panel-replace-row">
          <button
            type="button"
            onClick={replaceAll}
            disabled={
              replacing || results.length === 0 || query.trim().length === 0
            }
            className="nexus-search-panel-replace-all"
          >
            {replacing
              ? 'Replacing…'
              : `Replace All (${totalHits} hit${totalHits === 1 ? '' : 's'})`}
          </button>
        </div>
      </form>

      {error && (
        <div className="nexus-search-panel-error" role="alert">
          {error}
        </div>
      )}

      {lastReplace && (
        <div className="nexus-search-panel-summary">
          Replaced {lastReplace.replacements_applied} occurrence
          {lastReplace.replacements_applied === 1 ? '' : 's'} in{' '}
          {lastReplace.files_changed} file
          {lastReplace.files_changed === 1 ? '' : 's'}
          {lastReplace.errors && lastReplace.errors.length > 0
            ? ` · ${lastReplace.errors.length} error${
                lastReplace.errors.length === 1 ? '' : 's'
              }`
            : ''}
          .
        </div>
      )}

      {searched && !loading && results.length === 0 && !error && (
        <p className="nexus-search-panel-empty">
          No matches in any file.
        </p>
      )}

      <ul className="nexus-search-panel-results">
        {results.map((file) => (
          <FileGroup
            key={file.relpath}
            file={file}
            expanded={expanded[file.relpath] ?? true}
            onToggle={() => toggleExpanded(file.relpath)}
            onOpen={() => openHit(file.relpath)}
            onReplaceFile={() => replaceFile(file.relpath)}
            replacing={replacing}
            replaceLabel={replacement.length > 0}
          />
        ))}
      </ul>
    </div>
  )
}

interface FileGroupProps {
  file: FileMatches
  expanded: boolean
  onToggle: () => void
  onOpen: () => void
  onReplaceFile: () => void
  replacing: boolean
  /** True when the user has typed something into the replace
   *  field. Drives whether the per-file "replace" button
   *  appears — without a target string the button would be a
   *  no-op. */
  replaceLabel: boolean
}

function FileGroup({
  file,
  expanded,
  onToggle,
  onOpen,
  onReplaceFile,
  replacing,
  replaceLabel,
}: FileGroupProps) {
  return (
    <li className="nexus-search-panel-group">
      <header className="nexus-search-panel-group-header">
        <button
          type="button"
          className="nexus-search-panel-group-toggle"
          onClick={onToggle}
          aria-expanded={expanded}
          title={expanded ? 'Collapse' : 'Expand'}
        >
          <Icon name={expanded ? 'chev' : 'chev'} size={12} />
        </button>
        <button
          type="button"
          className="nexus-search-panel-group-path"
          onClick={onOpen}
          title="Open file"
        >
          <code>{file.relpath}</code>
        </button>
        <span className="nexus-search-panel-group-count">
          {file.hits.length}
        </span>
        {replaceLabel && (
          <button
            type="button"
            className="nexus-search-panel-group-replace"
            onClick={onReplaceFile}
            disabled={replacing}
            title="Apply replacement to this file only"
          >
            Replace
          </button>
        )}
      </header>
      {expanded && (
        <ul className="nexus-search-panel-group-rows">
          {file.hits.map((hit, idx) => (
            <HitRow
              key={`${hit.line}-${hit.column}-${idx}`}
              hit={hit}
              onOpen={onOpen}
            />
          ))}
        </ul>
      )}
    </li>
  )
}

interface HitRowProps {
  hit: LineMatch
  onOpen: () => void
}

function HitRow({ hit, onOpen }: HitRowProps) {
  // Render the matched span highlighted inline. CSS handles colour;
  // the structure here is `<span>before</span><mark>match</mark><span>after</span>`
  // so a screenreader and a normal copy-paste both behave naturally.
  const before = hit.text.slice(0, hit.column)
  const match = hit.text.slice(hit.column, hit.column + hit.length)
  const after = hit.text.slice(hit.column + hit.length)
  return (
    <li className="nexus-search-panel-hit">
      <button
        type="button"
        className="nexus-search-panel-hit-row"
        onClick={onOpen}
        title={`Open at line ${hit.line}`}
      >
        <span className="nexus-search-panel-hit-line">{hit.line}</span>
        <code className="nexus-search-panel-hit-text">
          {before}
          <mark>{match}</mark>
          {after}
        </code>
      </button>
    </li>
  )
}
