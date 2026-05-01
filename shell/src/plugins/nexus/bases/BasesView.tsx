// Top-level view for an open `.bases` directory. Loads via
// `base_load` on mount, then renders a view switcher + the currently
// active view (Table / Board / List). Phase-3 addition — switcher is
// in-memory; Phase 5 wires it through `base_view_*` persistence.

import { useEffect, useRef } from 'react'
import { useBasesStore, type ViewMode } from './basesStore'
import type { BasesKernelClient } from './kernelClient'
import { BasesTable } from './BasesTable'
import { BasesBoard } from './BasesBoard'
import { BasesList } from './BasesList'
import { BasesCalendar } from './BasesCalendar'
import { BasesGallery } from './BasesGallery'
import { BasesTimeline } from './BasesTimeline'
import { BasesViewBar } from './BasesViewBar'
import { SchemaEditor } from './SchemaEditor'
import { setActiveBases, type BasesHandle } from './activeBases'
import { withActiveTableClipboard } from './tableClipboard'
import { contextKeyService } from '../../../host/ContextKeyService'

interface Props {
  relpath: string
  client: BasesKernelClient
}

const VIEW_OPTIONS: { mode: ViewMode; label: string }[] = [
  { mode: 'table', label: 'Table' },
  { mode: 'board', label: 'Board' },
  { mode: 'list', label: 'List' },
  { mode: 'calendar', label: 'Calendar' },
  { mode: 'gallery', label: 'Gallery' },
  { mode: 'timeline', label: 'Timeline' },
]

export function BasesView({ relpath, client }: Props) {
  const tab = useBasesStore((s) => s.tabs[relpath])
  const ensureTab = useBasesStore((s) => s.ensureTab)
  const setLoading = useBasesStore((s) => s.setLoading)
  const setError = useBasesStore((s) => s.setError)
  const setBase = useBasesStore((s) => s.setBase)
  const setViewMode = useBasesStore((s) => s.setViewMode)
  const setReadOnly = useBasesStore((s) => s.setReadOnly)
  const schemaEditorOpen = useBasesStore((s) => s.tabs[relpath]?.schemaEditorOpen ?? false)
  const setSchemaEditorOpen = useBasesStore((s) => s.setSchemaEditorOpen)
  const trashOpen = useBasesStore((s) => s.tabs[relpath]?.trashOpen ?? false)
  const setTrashOpen = useBasesStore((s) => s.setTrashOpen)
  const undoLen = useBasesStore((s) => s.tabs[relpath]?.undoStack.length ?? 0)
  const redoLen = useBasesStore((s) => s.tabs[relpath]?.redoStack.length ?? 0)
  const lastUndoError = useBasesStore((s) => s.tabs[relpath]?.lastUndoError ?? null)
  const setLastUndoError = useBasesStore((s) => s.setLastUndoError)
  const undo = useBasesStore((s) => s.undo)
  const redo = useBasesStore((s) => s.redo)

  // Container ref used both by the focus-tracking effect (publishes a
  // BasesHandle on focusin) and as the keyboard target for the
  // table/list views nested inside.
  const containerRef = useRef<HTMLDivElement>(null)

  // Single-file Obsidian `.base` files take a different path through
  // the kernel (see ADR 0019). Detect by extension and route to the
  // read-only loader; everything else uses the existing `.bases`
  // directory loader.
  const isObsidianBase = relpath.toLowerCase().endsWith('.base')

  useEffect(() => {
    ensureTab(relpath)
    let cancelled = false
    setLoading(relpath, true)
    const promise = isObsidianBase
      ? client.loadObsidianBase(relpath).then((load) => {
          if (cancelled) return
          setBase(relpath, load.base)
          setReadOnly(relpath, true, load.unsupportedFilters)
        })
      : client.loadBase(relpath).then((base) => {
          if (cancelled) return
          setBase(relpath, base)
          setReadOnly(relpath, false, [])
        })
    promise.catch((err: unknown) => {
      if (cancelled) return
      const msg = err instanceof Error ? err.message : String(err)
      setError(relpath, msg)
    })
    return () => {
      cancelled = true
    }
  }, [relpath, client, ensureTab, setLoading, setBase, setError, setReadOnly, isObsidianBase])

  // Publish a BasesHandle whenever this leaf owns focus. Mirrors the
  // canvas active-handle pattern (CanvasView.tsx:1054-1107). The
  // shell's global keybinding dispatcher gates the Mod-Z / Mod-Y
  // chords on `bases.focused`, so chords only fire when the user is
  // actually inside a bases leaf — never when they're typing in
  // CodeMirror or the file tree.
  //
  // cut/copy/paste are stubs in BL-030; BL-031 wires them up to
  // cell-range clipboard ops.
  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    const handle: BasesHandle = {
      undo: () => {
        void useBasesStore.getState().undo(relpath)
      },
      redo: () => {
        void useBasesStore.getState().redo(relpath)
      },
      // Delegate to the active table-clipboard handle (registered by
      // BasesTable). Silent no-op when the table isn't mounted —
      // matches the canvas/active-canvas idiom for command-palette
      // invocations from a non-table view.
      cut: () => {
        withActiveTableClipboard((h) => h.cut())
      },
      copy: () => {
        withActiveTableClipboard((h) => h.copy())
      },
      paste: () => {
        withActiveTableClipboard((h) => h.paste())
      },
    }
    const claimFocus = () => {
      setActiveBases(handle)
      contextKeyService.set('bases.focused', true)
    }
    const releaseFocus = () => {
      setActiveBases(null)
      contextKeyService.set('bases.focused', false)
    }
    container.addEventListener('focusin', claimFocus)
    const onFocusOut = (e: FocusEvent) => {
      const next = e.relatedTarget as Node | null
      if (next && container.contains(next)) return
      releaseFocus()
    }
    container.addEventListener('focusout', onFocusOut)
    return () => {
      releaseFocus()
      container.removeEventListener('focusin', claimFocus)
      container.removeEventListener('focusout', onFocusOut)
    }
  }, [relpath])

  const wrapperStyle: React.CSSProperties = {
    width: '100%',
    height: '100%',
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    overflow: 'hidden',
    background: 'var(--background-primary)',
    color: 'var(--text-normal)',
    fontSize: 13,
    // Cleared default focus ring; the global keybinding dispatcher
    // hooks `bases.focused` off of focusin/out on this container.
    outline: 'none',
  }
  const headerStyle: React.CSSProperties = {
    padding: '8px 12px',
    borderBottom: '1px solid var(--background-modifier-border)',
    display: 'flex',
    alignItems: 'center',
    // Allow the toolbar to reflow onto a second row when the leaf is
    // narrowed by the right sidedock — otherwise the trailing buttons
    // (Trash/Schema/view-mode picker) get clipped at the dock edge and
    // look like they're hiding under the right panel.
    flexWrap: 'wrap',
    rowGap: 6,
    gap: 8,
    fontSize: 12,
    color: 'var(--text-muted)',
  }
  const bodyStyle: React.CSSProperties = {
    padding: 16,
    flex: 1,
    overflow: 'auto',
  }

  if (!tab || tab.loading) {
    return (
      <div ref={containerRef} tabIndex={0} style={wrapperStyle}>
        <div style={headerStyle}>{relpath}</div>
        <div style={bodyStyle}>Loading…</div>
      </div>
    )
  }
  if (tab.error) {
    return (
      <div ref={containerRef} tabIndex={0} style={wrapperStyle}>
        <div style={headerStyle}>{relpath}</div>
        <div style={{ ...bodyStyle, color: 'var(--risk)' }}>
          Failed to load base: {tab.error}
        </div>
      </div>
    )
  }
  const base = tab.base
  if (!base) {
    return (
      <div ref={containerRef} tabIndex={0} style={wrapperStyle}>
        <div style={headerStyle}>{relpath}</div>
        <div style={bodyStyle}>No base loaded.</div>
      </div>
    )
  }
  const fieldCount = Object.keys(base.schema.fields).length
  const mode = tab.viewMode
  const readOnly = tab.readOnly
  const unsupportedFilters = tab.unsupportedFilters ?? []
  // Soft-deleted records stay on disk. The default ("live") set
  // hides them; the trash filter inverts that so the user can
  // restore or permanently delete from a single view. WI-10 §4.2
  // acceptance — soft-deleted records reachable via UI, not just API.
  const liveRecords = base.records.filter((r) => !r.deletedAt)
  const trashedRecords = base.records.filter((r) => !!r.deletedAt)
  const visibleRecords = trashOpen ? trashedRecords : liveRecords
  const visibleBase = { ...base, records: visibleRecords }
  const trashCount = trashedRecords.length
  return (
    <div ref={containerRef} tabIndex={0} style={wrapperStyle}>
      <div style={headerStyle}>
        <strong style={{ color: 'var(--text-normal)' }}>{base.name}</strong>
        <span>·</span>
        <span>{visibleRecords.length} records</span>
        <span>·</span>
        <span>{fieldCount} fields</span>
        <span>·</span>
        <span>{base.views.length} views</span>
        {readOnly && (
          <span
            title="This is an Obsidian .base file — read-only (ADR 0019)"
            style={{
              padding: '2px 8px',
              background: 'var(--background-secondary)',
              border: '1px solid var(--background-modifier-border)',
              borderRadius: 3,
              fontSize: 10,
              color: 'var(--text-muted)',
              textTransform: 'uppercase',
              letterSpacing: 0.5,
            }}
          >
            Read-only
          </span>
        )}
        <div style={{ flex: 1 }} />
        {!readOnly && (
          <>
            <button
              type="button"
              disabled={undoLen === 0}
              onClick={() => void undo(relpath)}
              title="Undo (Ctrl/Cmd+Z)"
              style={{
                padding: '3px 10px',
                background: 'var(--background-secondary)',
                color: 'var(--text-normal)',
                border: '1px solid var(--background-modifier-border)',
                borderRadius: 3,
                fontSize: 11,
                cursor: undoLen === 0 ? 'not-allowed' : 'pointer',
                opacity: undoLen === 0 ? 0.4 : 1,
                marginRight: 4,
              }}
            >
              Undo{undoLen > 0 ? ` (${undoLen})` : ''}
            </button>
            <button
              type="button"
              disabled={redoLen === 0}
              onClick={() => void redo(relpath)}
              title="Redo (Ctrl/Cmd+Shift+Z)"
              style={{
                padding: '3px 10px',
                background: 'var(--background-secondary)',
                color: 'var(--text-normal)',
                border: '1px solid var(--background-modifier-border)',
                borderRadius: 3,
                fontSize: 11,
                cursor: redoLen === 0 ? 'not-allowed' : 'pointer',
                opacity: redoLen === 0 ? 0.4 : 1,
                marginRight: 8,
              }}
            >
              Redo{redoLen > 0 ? ` (${redoLen})` : ''}
            </button>
          </>
        )}
        {!readOnly && <button
          type="button"
          onClick={() => setTrashOpen(relpath, !trashOpen)}
          title={
            trashOpen
              ? 'Back to live records'
              : `Show soft-deleted records (${trashCount})`
          }
          style={{
            padding: '3px 10px',
            background: trashOpen ? 'var(--risk)' : 'var(--background-secondary)',
            color: trashOpen ? 'var(--interactive-accent-ink)' : 'var(--text-normal)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 3,
            fontSize: 11,
            cursor: 'pointer',
            marginRight: 8,
          }}
        >
          {trashOpen ? `← Live (${liveRecords.length})` : `Trash${trashCount > 0 ? ` (${trashCount})` : ''}`}
        </button>}
        {!readOnly && <button
          type="button"
          onClick={() => setSchemaEditorOpen(relpath, !schemaEditorOpen)}
          title="Schema editor"
          style={{
            padding: '3px 10px',
            background: schemaEditorOpen ? 'var(--interactive-accent)' : 'var(--background-secondary)',
            color: schemaEditorOpen ? 'var(--interactive-accent-ink)' : 'var(--text-normal)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 3,
            fontSize: 11,
            cursor: 'pointer',
            marginRight: 8,
          }}
        >
          Schema
        </button>}
        <div style={{ display: 'flex', gap: 4 }}>
          {VIEW_OPTIONS.map((opt) => {
            const active = opt.mode === mode
            return (
              <button
                key={opt.mode}
                type="button"
                onClick={() => setViewMode(relpath, opt.mode)}
                style={{
                  padding: '3px 10px',
                  background: active
                    ? 'var(--interactive-accent)'
                    : 'var(--background-secondary)',
                  color: active ? 'var(--interactive-accent-ink)' : 'var(--text-normal)',
                  border: '1px solid var(--background-modifier-border)',
                  borderRadius: 3,
                  fontSize: 11,
                  cursor: 'pointer',
                }}
              >
                {opt.label}
              </button>
            )
          })}
        </div>
      </div>
      {lastUndoError && (
        <div
          role="alert"
          style={{
            padding: '6px 12px',
            borderBottom: '1px solid var(--background-modifier-border)',
            background: 'var(--background-secondary)',
            color: 'var(--risk)',
            fontSize: 11,
            display: 'flex',
            alignItems: 'center',
            gap: 8,
          }}
        >
          <span style={{ flex: 1 }}>{lastUndoError}</span>
          <button
            type="button"
            onClick={() => setLastUndoError(relpath, null)}
            title="Dismiss"
            style={{
              padding: '2px 8px',
              background: 'transparent',
              color: 'var(--text-muted)',
              border: '1px solid var(--background-modifier-border)',
              borderRadius: 3,
              fontSize: 11,
              cursor: 'pointer',
            }}
          >
            Dismiss
          </button>
        </div>
      )}
      {unsupportedFilters.length > 0 && (
        <div
          role="alert"
          style={{
            padding: '6px 12px',
            borderBottom: '1px solid var(--background-modifier-border)',
            background: 'var(--background-secondary)',
            color: 'var(--risk)',
            fontSize: 11,
          }}
        >
          {unsupportedFilters.length === 1
            ? '1 filter expression is unsupported and was excluded:'
            : `${unsupportedFilters.length} filter expressions are unsupported and were excluded:`}
          <ul style={{ margin: '4px 0 0 16px', padding: 0 }}>
            {unsupportedFilters.map((f) => (
              <li key={f} style={{ fontFamily: 'var(--font-monospace, monospace)' }}>{f}</li>
            ))}
          </ul>
        </div>
      )}
      <BasesViewBar relpath={relpath} base={visibleBase} client={client} />
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          {trashOpen && trashCount === 0 ? (
            <TrashEmptyState onBackToLive={() => setTrashOpen(relpath, false)} />
          ) : (
            <>
              {mode === 'table' && <BasesTable relpath={relpath} base={visibleBase} client={client} />}
              {mode === 'board' && <BasesBoard relpath={relpath} base={visibleBase} client={client} />}
              {mode === 'list' && <BasesList relpath={relpath} base={visibleBase} client={client} />}
              {mode === 'calendar' && <BasesCalendar relpath={relpath} base={visibleBase} client={client} />}
              {mode === 'gallery' && <BasesGallery relpath={relpath} base={visibleBase} client={client} />}
              {mode === 'timeline' && <BasesTimeline relpath={relpath} base={visibleBase} client={client} />}
            </>
          )}
        </div>
        {schemaEditorOpen && <SchemaEditor relpath={relpath} base={base} client={client} />}
      </div>
    </div>
  )
}

/** Empty-state placeholder shown when the trash filter is active but
 *  no records are soft-deleted. The non-table views can render in
 *  surprising ways with an empty record set (calendar shows empty
 *  months, timeline shows empty lanes); this short-circuits those
 *  before they mount. */
function TrashEmptyState({ onBackToLive }: { onBackToLive: () => void }) {
  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 12,
        padding: 24,
        color: 'var(--text-muted)',
        fontSize: 13,
      }}
    >
      <div style={{ fontSize: 13, opacity: 0.7 }}>The trash is empty.</div>
      <button
        type="button"
        onClick={onBackToLive}
        style={{
          padding: '4px 12px',
          background: 'var(--background-secondary)',
          color: 'var(--text-normal)',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 3,
          fontSize: 11,
          cursor: 'pointer',
        }}
      >
        ← Back to live records
      </button>
    </div>
  )
}
