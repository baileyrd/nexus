// shell/src/plugins/nexus/recall/RecallOverlay.tsx
//
// BL-044 — recall overlay UI.
//
// Visual scaffold mirrors `ai/CmdIOverlay.tsx` (chrome `overlay` slot,
// centred dialog, backdrop click-to-dismiss, autofocus on input). The
// content adapter is different: the body is a results list rather than
// a streaming response panel, and Enter/Cmd+Enter route to insert /
// copy actions instead of submitting a chat turn.
//
// Keymap inside the overlay (handled by the textarea + result list,
// NOT by the global keymap registry — Esc inside an overlay shouldn't
// require a context-key dance for in-window focus):
//
//   Esc                 → close
//   ArrowUp / ArrowDown → moveSelection(-1 / +1)
//   Enter               → insert at editor caret + close
//   Cmd/Ctrl+Enter      → copy formatted snippet to clipboard + close

import { useEffect, useMemo, useRef } from 'react'
import { useRecallStore } from './recallStore'
import { applyCodeFilter, applyLanguageFilter, availableLanguages } from './codeFilter'
import {
  cancelPendingSearch,
  copySelectedSnippet,
  insertSelectedAsLink,
  insertSelectedSnippet,
  searchDebounced,
} from './recallRuntime'
import { getRecallApi } from './recallApi'

/** Forward-slash basename helper (kept inline to avoid an import for one
 *  line — used for the result-row caption). */
function basename(p: string): string {
  const i = p.lastIndexOf('/')
  return i === -1 ? p : p.slice(i + 1)
}

/** AIG-06 — split `text` into runs that either match one of the
 *  whitespace-separated `query` terms (case-insensitive) or fill the
 *  gaps between matches. Empty / whitespace-only queries pass through
 *  as a single non-matching run so the preview renders unchanged.
 *
 *  Exported for unit testing the splitter independently of React.
 */
export function highlightRuns(
  text: string,
  query: string,
): Array<{ text: string; match: boolean }> {
  const terms = query
    .split(/\s+/)
    .map((t) => t.trim())
    .filter((t) => t.length > 0)
  if (terms.length === 0) return [{ text, match: false }]
  const escaped = terms.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  const re = new RegExp(`(${escaped.join('|')})`, 'gi')
  const out: Array<{ text: string; match: boolean }> = []
  let last = 0
  let m: RegExpExecArray | null
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) {
      out.push({ text: text.slice(last, m.index), match: false })
    }
    out.push({ text: m[0], match: true })
    last = m.index + m[0].length
    // Defensive: zero-length matches would loop forever. Bump.
    if (m.index === re.lastIndex) re.lastIndex += 1
  }
  if (last < text.length) {
    out.push({ text: text.slice(last), match: false })
  }
  return out
}

function HighlightedText({ text, query }: { text: string; query: string }) {
  const runs = useMemo(() => highlightRuns(text, query), [text, query])
  return (
    <>
      {runs.map((r, i) =>
        r.match ? (
          <mark
            key={i}
            style={{
              background: 'var(--text-highlight, var(--interactive-accent-soft))',
              color: 'inherit',
              padding: '0 1px',
              borderRadius: 2,
            }}
          >
            {r.text}
          </mark>
        ) : (
          <span key={i}>{r.text}</span>
        ),
      )}
    </>
  )
}

function ResultRow({
  filePath,
  excerpt,
  selected,
  onClick,
}: {
  filePath: string
  excerpt: string
  selected: boolean
  onClick: () => void
}) {
  return (
    <li
      role="option"
      aria-selected={selected}
      onClick={onClick}
      style={{
        padding: '8px 16px',
        cursor: 'pointer',
        background: selected ? 'var(--bg-selected, var(--interactive-accent-soft))' : 'transparent',
        borderBottom: '1px solid var(--divider-color)',
        fontFamily: 'var(--font-interface)',
        fontSize: 13,
        color: 'var(--text-normal)',
      }}
    >
      <div style={{ fontWeight: 600, marginBottom: 2 }}>
        {basename(filePath)}
      </div>
      <div
        style={{
          color: 'var(--text-muted)',
          fontSize: 12,
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
        }}
      >
        {excerpt.replace(/\s+/g, ' ').trim()}
      </div>
    </li>
  )
}

function ResultList() {
  const rawResults = useRecallStore((s) => s.results)
  const codeOnly = useRecallStore((s) => s.codeOnly)
  const selectedLanguages = useRecallStore((s) => s.selectedLanguages)
  const selectedIndex = useRecallStore((s) => s.selectedIndex)
  const status = useRecallStore((s) => s.status)
  const error = useRecallStore((s) => s.error)
  const setSelectedIndex = useRecallStore((s) => s.setSelectedIndex)
  // BL-046 phase 2 — code-only filter chip applied at render
  // time. Keeping `results` in the store unfiltered means toggling
  // a chip off restores the prior list without a re-fetch. Phase 3
  // composes the language refinement on top.
  const results = applyLanguageFilter(
    applyCodeFilter(rawResults, codeOnly),
    selectedLanguages,
  )

  if (status === 'error' && error) {
    return (
      <div
        role="alert"
        style={{
          padding: '12px 16px',
          color: 'var(--danger)',
          fontFamily: 'var(--font-interface)',
          fontSize: 13,
          borderTop: '1px solid var(--divider-color)',
        }}
      >
        {error.message}
      </div>
    )
  }

  if (results.length === 0) {
    return (
      <div
        style={{
          padding: '12px 16px',
          color: 'var(--text-faint)',
          fontFamily: 'var(--font-interface)',
          fontSize: 13,
          borderTop: '1px solid var(--divider-color)',
        }}
      >
        {status === 'searching' ? 'Searching…' : 'Type to recall from your capture notes.'}
      </div>
    )
  }

  return (
    <ul
      role="listbox"
      aria-label="Recall results"
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        maxHeight: 320,
        overflowY: 'auto',
        borderTop: '1px solid var(--divider-color)',
      }}
    >
      {results.map((m, i) => (
        <ResultRow
          key={`${m.file_path}:${m.block_id ?? i}`}
          filePath={m.file_path}
          excerpt={m.chunk_text}
          selected={i === selectedIndex}
          onClick={() => setSelectedIndex(i)}
        />
      ))}
    </ul>
  )
}

export function RecallOverlay() {
  const visible = useRecallStore((s) => s.visible)
  const query = useRecallStore((s) => s.query)
  const setQuery = useRecallStore((s) => s.setQuery)
  const close = useRecallStore((s) => s.close)
  const moveSelection = useRecallStore((s) => s.moveSelection)

  const inputRef = useRef<HTMLInputElement | null>(null)

  // Autofocus on each open. Same RAF dodge as the Cmd+I overlay.
  useEffect(() => {
    if (!visible) return
    const id = requestAnimationFrame(() => inputRef.current?.focus())
    return () => cancelAnimationFrame(id)
  }, [visible])

  // Cancel any pending debounced search when the overlay closes so a
  // late timer can't repopulate a freshly-closed overlay.
  useEffect(() => {
    if (!visible) cancelPendingSearch()
  }, [visible])

  if (!visible) return null

  const onChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const next = e.target.value
    setQuery(next)
    let api
    try {
      api = getRecallApi()
    } catch {
      return
    }
    void searchDebounced(api, next)
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      cancelPendingSearch()
      close()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      moveSelection(+1)
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      moveSelection(-1)
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      if (e.metaKey || e.ctrlKey) {
        // Cmd/Ctrl+Enter → clipboard. Close after copy regardless of
        // success — the user's intent was to dismiss + carry the
        // snippet elsewhere.
        void copySelectedSnippet().finally(() => close())
        return
      }
      if (e.shiftKey) {
        // AIG-06 — Shift+Enter → bare wikilink at editor caret.
        // Useful when you want to reference the source note without
        // copying its body into the current document.
        insertSelectedAsLink()
        close()
        return
      }
      // Plain Enter → insert quoted snippet at editor caret. If no
      // editor is active the splice is a silent no-op; we still close
      // so the user isn't stuck inside a half-functional overlay.
      insertSelectedSnippet()
      close()
      return
    }
  }

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) close()
  }

  return (
    <div
      onClick={onBackdropClick}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'oklch(0 0 0 / 0.35)',
        pointerEvents: 'auto',
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'flex-start',
        paddingTop: 120,
      }}
    >
      <div
        role="dialog"
        aria-label="Recall from capture notes"
        style={{
          width: 880,
          maxWidth: '92vw',
          background: 'var(--background-secondary)',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 'var(--radius-l)',
          boxShadow: 'var(--shadow)',
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <input
          ref={inputRef}
          value={query}
          onChange={onChange}
          onKeyDown={onKeyDown}
          placeholder="Recall from your capture notes…"
          spellCheck={false}
          autoComplete="off"
          style={{
            background: 'transparent',
            border: 0,
            outline: 0,
            color: 'var(--text-normal)',
            fontFamily: 'var(--font-interface)',
            fontSize: 14,
            padding: '12px 16px',
          }}
        />
        <FilterChips />
        <div
          style={{
            display: 'flex',
            borderTop: '1px solid var(--divider-color)',
            // Split: list 40%, preview 60%. Both panes scroll
            // independently so a long preview doesn't push the list
            // out of view.
          }}
        >
          <div style={{ flex: '0 0 40%', minWidth: 0, borderRight: '1px solid var(--divider-color)' }}>
            <ResultList />
          </div>
          <div style={{ flex: '1 1 auto', minWidth: 0 }}>
            <PreviewPane />
          </div>
        </div>
        <ActionFooter onClose={close} />
      </div>
    </div>
  )
}

// ── AIG-06 — preview pane + action footer ──────────────────────────

function PreviewPane() {
  const rawResults = useRecallStore((s) => s.results)
  const codeOnly = useRecallStore((s) => s.codeOnly)
  const selectedLanguages = useRecallStore((s) => s.selectedLanguages)
  const selectedIndex = useRecallStore((s) => s.selectedIndex)
  const query = useRecallStore((s) => s.query)
  const results = applyLanguageFilter(
    applyCodeFilter(rawResults, codeOnly),
    selectedLanguages,
  )
  const match = results[selectedIndex]
  if (!match) {
    return (
      <div
        style={{
          padding: 16,
          color: 'var(--text-faint)',
          fontFamily: 'var(--font-interface)',
          fontSize: 12,
          maxHeight: 320,
          minHeight: 120,
          overflow: 'auto',
        }}
      >
        Select a result to preview it.
      </div>
    )
  }
  return (
    <div
      style={{
        padding: '12px 16px',
        maxHeight: 320,
        overflow: 'auto',
        fontFamily: 'var(--font-interface)',
      }}
    >
      <div
        style={{
          fontSize: 11,
          color: 'var(--text-faint)',
          marginBottom: 8,
          fontFamily: 'var(--font-monospace)',
          wordBreak: 'break-all',
        }}
      >
        {match.file_path}
      </div>
      <pre
        style={{
          margin: 0,
          fontFamily: 'var(--font-monospace)',
          fontSize: 12,
          lineHeight: 1.5,
          color: 'var(--text-normal)',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
        }}
      >
        <HighlightedText text={match.chunk_text} query={query} />
      </pre>
    </div>
  )
}

function ActionFooter({ onClose }: { onClose: () => void }) {
  const rawResults = useRecallStore((s) => s.results)
  const codeOnly = useRecallStore((s) => s.codeOnly)
  const selectedLanguages = useRecallStore((s) => s.selectedLanguages)
  const selectedIndex = useRecallStore((s) => s.selectedIndex)
  const visibleResults = applyLanguageFilter(
    applyCodeFilter(rawResults, codeOnly),
    selectedLanguages,
  )
  const hasSelection = visibleResults[selectedIndex] != null

  const onInsertQuote = () => {
    insertSelectedSnippet()
    onClose()
  }
  const onInsertLink = () => {
    insertSelectedAsLink()
    onClose()
  }
  const onCopy = () => {
    void copySelectedSnippet().finally(onClose)
  }

  return (
    <div
      style={{
        display: 'flex',
        gap: 8,
        padding: '8px 12px',
        borderTop: '1px solid var(--divider-color)',
        background: 'var(--background-primary)',
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
      }}
    >
      <span style={{ marginRight: 'auto', color: 'var(--text-faint)', alignSelf: 'center' }}>
        <kbd>Enter</kbd> quote · <kbd>Shift+Enter</kbd> link · <kbd>⌘Enter</kbd> copy
      </span>
      <button
        type="button"
        disabled={!hasSelection}
        onClick={onInsertQuote}
        title="Insert a markdown blockquote of the chunk with a wikilink footer"
        style={footerButtonStyle(hasSelection, true)}
        data-testid="recall-insert-quote"
      >
        Insert as quote
      </button>
      <button
        type="button"
        disabled={!hasSelection}
        onClick={onInsertLink}
        title="Insert a bare [[wikilink]] to the source note"
        style={footerButtonStyle(hasSelection, false)}
        data-testid="recall-insert-link"
      >
        Insert as link
      </button>
      <button
        type="button"
        disabled={!hasSelection}
        onClick={onCopy}
        title="Copy the formatted snippet to the clipboard"
        style={footerButtonStyle(hasSelection, false)}
        data-testid="recall-copy"
      >
        Copy
      </button>
    </div>
  )
}

function footerButtonStyle(enabled: boolean, primary: boolean): React.CSSProperties {
  return {
    padding: '4px 10px',
    background: primary && enabled ? 'var(--interactive-accent)' : 'transparent',
    color: primary && enabled
      ? 'var(--interactive-accent-ink, white)'
      : 'var(--text-normal)',
    border: '1px solid var(--background-modifier-border)',
    borderRadius: 4,
    fontFamily: 'var(--font-interface)',
    fontSize: 12,
    cursor: enabled ? 'pointer' : 'not-allowed',
    opacity: enabled ? 1 : 0.55,
  }
}

/** BL-046 phase 2/3 — filter-chip row above the result list. The
 *  parent "From project" chip narrows to code captures; below it,
 *  phase-3 language pills appear for each language present in the
 *  code-only-filtered result set. Each language pill toggles
 *  inclusion in `selectedLanguages` (OR semantics). The pills only
 *  show when "From project" is on so the chip row has a coherent
 *  hierarchy — flipping the parent chip off also wipes any active
 *  language refinement (handled in `setCodeOnly`). */
function FilterChips() {
  const rawResults = useRecallStore((s) => s.results)
  const codeOnly = useRecallStore((s) => s.codeOnly)
  const setCodeOnly = useRecallStore((s) => s.setCodeOnly)
  const selectedLanguages = useRecallStore((s) => s.selectedLanguages)
  const toggleLanguage = useRecallStore((s) => s.toggleLanguage)

  // Languages are derived from the code-only-filtered slice — that
  // way every pill corresponds to an actual filterable language in
  // the *currently visible* result set.
  const languages = codeOnly
    ? availableLanguages(applyCodeFilter(rawResults, true))
    : []

  return (
    <div
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 6,
        padding: '0 16px 8px',
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
      }}
    >
      <button
        type="button"
        aria-pressed={codeOnly}
        onClick={() => setCodeOnly(!codeOnly)}
        title="Limit results to code captures (BL-046)"
        style={{
          padding: '2px 10px',
          borderRadius: 999,
          border: '1px solid var(--divider-color)',
          background: codeOnly ? 'var(--interactive-accent)' : 'var(--background-secondary)',
          color: codeOnly ? 'var(--interactive-accent-ink)' : 'var(--text-normal)',
          cursor: 'pointer',
        }}
      >
        From project
      </button>
      {languages.map((lang) => {
        const active = selectedLanguages.includes(lang)
        return (
          <button
            key={lang}
            type="button"
            aria-pressed={active}
            onClick={() => toggleLanguage(lang)}
            title={`Limit code captures to ${lang}`}
            style={{
              padding: '2px 10px',
              borderRadius: 999,
              border: '1px solid var(--divider-color)',
              background: active ? 'var(--interactive-accent)' : 'var(--background-secondary)',
              color: active ? 'var(--interactive-accent-ink)' : 'var(--text-normal)',
              cursor: 'pointer',
            }}
          >
            {lang}
          </button>
        )
      })}
    </div>
  )
}
