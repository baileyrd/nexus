// Top-level view for an open `.bases` directory. Loads via
// `base_load` on mount, then renders a view switcher + the currently
// active view (Table / Board / List). Phase-3 addition — switcher is
// in-memory; Phase 5 wires it through `base_view_*` persistence.

import { useEffect } from 'react'
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
  const schemaEditorOpen = useBasesStore((s) => s.tabs[relpath]?.schemaEditorOpen ?? false)
  const setSchemaEditorOpen = useBasesStore((s) => s.setSchemaEditorOpen)

  useEffect(() => {
    ensureTab(relpath)
    let cancelled = false
    setLoading(relpath, true)
    client
      .loadBase(relpath)
      .then((base) => {
        if (cancelled) return
        setBase(relpath, base)
      })
      .catch((err: unknown) => {
        if (cancelled) return
        const msg = err instanceof Error ? err.message : String(err)
        setError(relpath, msg)
      })
    return () => {
      cancelled = true
    }
  }, [relpath, client, ensureTab, setLoading, setBase, setError])

  const wrapperStyle: React.CSSProperties = {
    height: '100%',
    display: 'flex',
    flexDirection: 'column',
    background: 'var(--bg-primary, #1e1e1e)',
    color: 'var(--fg-primary, #e4e4e7)',
    fontSize: 13,
  }
  const headerStyle: React.CSSProperties = {
    padding: '8px 12px',
    borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    fontSize: 12,
    color: 'var(--fg-muted, #9ca3af)',
  }
  const bodyStyle: React.CSSProperties = {
    padding: 16,
    flex: 1,
    overflow: 'auto',
  }

  if (!tab || tab.loading) {
    return (
      <div style={wrapperStyle}>
        <div style={headerStyle}>{relpath}</div>
        <div style={bodyStyle}>Loading…</div>
      </div>
    )
  }
  if (tab.error) {
    return (
      <div style={wrapperStyle}>
        <div style={headerStyle}>{relpath}</div>
        <div style={{ ...bodyStyle, color: 'var(--risk, #f48771)' }}>
          Failed to load base: {tab.error}
        </div>
      </div>
    )
  }
  const base = tab.base
  if (!base) {
    return (
      <div style={wrapperStyle}>
        <div style={headerStyle}>{relpath}</div>
        <div style={bodyStyle}>No base loaded.</div>
      </div>
    )
  }
  const fieldCount = Object.keys(base.schema.fields).length
  const mode = tab.viewMode
  return (
    <div style={wrapperStyle}>
      <div style={headerStyle}>
        <strong style={{ color: 'var(--fg-primary, #e4e4e7)' }}>{base.name}</strong>
        <span>·</span>
        <span>{base.records.length} records</span>
        <span>·</span>
        <span>{fieldCount} fields</span>
        <span>·</span>
        <span>{base.views.length} views</span>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          onClick={() => setSchemaEditorOpen(relpath, !schemaEditorOpen)}
          title="Schema editor"
          style={{
            padding: '3px 10px',
            background: schemaEditorOpen ? 'var(--accent, #60a5fa)' : 'var(--bg-raised, #252529)',
            color: schemaEditorOpen ? 'var(--fg-on-accent, #0f1117)' : 'var(--fg-primary, #e4e4e7)',
            border: '1px solid var(--border-subtle, #2a2a2e)',
            borderRadius: 3,
            fontSize: 11,
            cursor: 'pointer',
            marginRight: 8,
          }}
        >
          Schema
        </button>
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
                    ? 'var(--accent, #60a5fa)'
                    : 'var(--bg-raised, #252529)',
                  color: active ? 'var(--fg-on-accent, #0f1117)' : 'var(--fg-primary, #e4e4e7)',
                  border: '1px solid var(--border-subtle, #2a2a2e)',
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
      <BasesViewBar relpath={relpath} base={base} client={client} />
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          {mode === 'table' && <BasesTable relpath={relpath} base={base} client={client} />}
          {mode === 'board' && <BasesBoard relpath={relpath} base={base} client={client} />}
          {mode === 'list' && <BasesList relpath={relpath} base={base} client={client} />}
          {mode === 'calendar' && <BasesCalendar relpath={relpath} base={base} client={client} />}
          {mode === 'gallery' && <BasesGallery relpath={relpath} base={base} client={client} />}
          {mode === 'timeline' && <BasesTimeline relpath={relpath} base={base} client={client} />}
        </div>
        {schemaEditorOpen && <SchemaEditor relpath={relpath} base={base} client={client} />}
      </div>
    </div>
  )
}
