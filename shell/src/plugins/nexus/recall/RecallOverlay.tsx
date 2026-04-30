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

import { useEffect, useRef } from 'react'
import { useRecallStore } from './recallStore'
import { applyCodeFilter, applyLanguageFilter, availableLanguages } from './codeFilter'
import {
  cancelPendingSearch,
  copySelectedSnippet,
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
        background: selected ? 'var(--bg-selected, var(--accent-soft))' : 'transparent',
        borderBottom: '1px solid var(--line-soft)',
        fontFamily: 'var(--f-ui)',
        fontSize: 13,
        color: 'var(--fg)',
      }}
    >
      <div style={{ fontWeight: 600, marginBottom: 2 }}>
        {basename(filePath)}
      </div>
      <div
        style={{
          color: 'var(--fg-muted)',
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
          color: 'var(--danger, #b00020)',
          fontFamily: 'var(--f-ui)',
          fontSize: 13,
          borderTop: '1px solid var(--line-soft)',
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
          color: 'var(--fg-dim)',
          fontFamily: 'var(--f-ui)',
          fontSize: 13,
          borderTop: '1px solid var(--line-soft)',
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
        borderTop: '1px solid var(--line-soft)',
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
      // Plain Enter → insert at editor caret. If no editor is active
      // the splice is a silent no-op; we still close so the user
      // isn't stuck inside a half-functional overlay.
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
          width: 640,
          maxWidth: '92vw',
          background: 'var(--bg-raised)',
          border: '1px solid var(--line)',
          borderRadius: 'var(--r-lg)',
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
            color: 'var(--fg)',
            fontFamily: 'var(--f-ui)',
            fontSize: 14,
            padding: '12px 16px',
          }}
        />
        <FilterChips />
        <ResultList />
      </div>
    </div>
  )
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
        fontFamily: 'var(--f-ui)',
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
          border: '1px solid var(--line-soft)',
          background: codeOnly ? 'var(--accent, #4c8bf5)' : 'var(--bg-raised)',
          color: codeOnly ? 'var(--bg-base, #fff)' : 'var(--fg)',
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
              border: '1px solid var(--line-soft)',
              background: active ? 'var(--accent, #4c8bf5)' : 'var(--bg-raised)',
              color: active ? 'var(--bg-base, #fff)' : 'var(--fg)',
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
