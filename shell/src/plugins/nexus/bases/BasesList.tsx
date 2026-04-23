// Phase 3 of docs/bases-shell-plan.md — List view. Groups records by
// a user-chosen property and renders each group as a collapsible
// section. Share the cell-formatting machinery with BasesTable so
// stored values render consistently across views.

import { useMemo } from 'react'
import { useBasesStore } from './basesStore'
import type { Base, BaseRecord, BasesKernelClient } from './kernelClient'
import { formatValue, parseFieldDef, typeGlyph, type FieldDefinition } from './fieldTypes'

interface Props {
  relpath: string
  base: Base
  /** Reserved for the Phase-3 list view — no record writes yet, but
   *  future additions (e.g. drag-to-reorder within group) will need
   *  the handle. */
  client: BasesKernelClient
}

interface Column {
  name: string
  def: FieldDefinition
}

const EMPTY_KEY = '__empty__'

export function BasesList({ relpath, base, client: _client }: Props) {
  const groupField = useBasesStore((s) => s.tabs[relpath]?.listGroupField ?? null)
  const collapsed = useBasesStore((s) => s.tabs[relpath]?.collapsedGroups ?? new Set<string>())
  const setListGroupField = useBasesStore((s) => s.setListGroupField)
  const toggleGroupCollapsed = useBasesStore((s) => s.toggleGroupCollapsed)
  const setSelectedRecordId = useBasesStore((s) => s.setSelectedRecordId)

  const columns = useMemo<Column[]>(
    () =>
      Object.entries(base.schema.fields ?? {})
        .filter(([n]) => n !== 'id')
        .map(([name, def]) => ({ name, def: parseFieldDef(def) })),
    [base],
  )

  const primary = useMemo(
    () => columns.find((c) => c.def.primary) ?? columns[0],
    [columns],
  )

  const active = useMemo<Column | null>(() => {
    if (groupField) {
      const match = columns.find((c) => c.name === groupField)
      if (match) return match
    }
    return primary ?? null
  }, [groupField, columns, primary])

  const detailColumns = useMemo(
    () => columns.filter((c) => c !== active).slice(0, 4),
    [columns, active],
  )

  const groups = useMemo(() => {
    if (!active) return []
    const buckets = new Map<string, BaseRecord[]>()
    for (const r of base.records) {
      const raw = r[active.name]
      const key = groupKeyFor(active, raw)
      const existing = buckets.get(key)
      if (existing) existing.push(r)
      else buckets.set(key, [r])
    }
    // Sort group keys alphabetically; push empty group last.
    return Array.from(buckets.entries())
      .sort(([a], [b]) => {
        if (a === EMPTY_KEY) return 1
        if (b === EMPTY_KEY) return -1
        return a.localeCompare(b)
      })
      .map(([key, records]) => ({ key, records }))
  }, [active, base.records])

  if (!active) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--fg-muted, #9ca3af)',
          fontSize: 12,
        }}
      >
        Add a field to see the List view.
      </div>
    )
  }

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '6px 12px',
          borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
          fontSize: 12,
          color: 'var(--fg-muted, #9ca3af)',
        }}
      >
        <span>Group by</span>
        <select
          value={active.name}
          onChange={(e) => setListGroupField(relpath, e.target.value || null)}
          style={{
            background: 'var(--bg-raised, #252529)',
            color: 'var(--fg-primary, #e4e4e7)',
            border: '1px solid var(--border-subtle, #2a2a2e)',
            borderRadius: 3,
            padding: '2px 6px',
            fontSize: 11,
          }}
        >
          {columns.map((c) => (
            <option key={c.name} value={c.name}>
              {typeGlyph(c.def.type)} {c.name}
            </option>
          ))}
        </select>
      </div>
      <div style={{ flex: 1, overflow: 'auto', padding: 12 }}>
        {groups.map(({ key, records }) => {
          const isCollapsed = collapsed.has(key)
          const label = key === EMPTY_KEY ? '(empty)' : groupKeyLabel(active, key)
          return (
            <div key={key} style={{ marginBottom: 8 }}>
              <button
                type="button"
                onClick={() => toggleGroupCollapsed(relpath, key)}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                  width: '100%',
                  background: 'var(--bg-raised, #252529)',
                  border: '1px solid var(--border-subtle, #2a2a2e)',
                  color: 'var(--fg-primary, #e4e4e7)',
                  borderRadius: 4,
                  padding: '6px 10px',
                  fontSize: 12,
                  cursor: 'pointer',
                  textAlign: 'left',
                }}
              >
                <span
                  style={{
                    display: 'inline-block',
                    width: 10,
                    color: 'var(--fg-dim, #6b7280)',
                    transform: isCollapsed ? 'rotate(0deg)' : 'rotate(90deg)',
                    transition: 'transform 80ms',
                  }}
                >
                  ▶
                </span>
                <strong>{label}</strong>
                <span style={{ color: 'var(--fg-dim, #6b7280)', marginLeft: 'auto' }}>
                  {records.length}
                </span>
              </button>
              {!isCollapsed && (
                <div
                  style={{
                    marginTop: 4,
                    display: 'flex',
                    flexDirection: 'column',
                  }}
                >
                  {records.map((r) => (
                    <ListRow
                      key={r.id}
                      record={r}
                      primary={primary}
                      detail={detailColumns}
                      onSelect={() => setSelectedRecordId(relpath, r.id)}
                    />
                  ))}
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

function ListRow({
  record,
  primary,
  detail,
  onSelect,
}: {
  record: BaseRecord
  primary: Column | undefined
  detail: Column[]
  onSelect(): void
}) {
  const title = primary ? formatValue(primary.def.type, record[primary.name]) : record.id
  return (
    <button
      type="button"
      onClick={onSelect}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        padding: '6px 12px',
        background: 'transparent',
        border: 'none',
        borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
        color: 'var(--fg-primary, #e4e4e7)',
        fontSize: 12,
        cursor: 'pointer',
        textAlign: 'left',
      }}
    >
      <span
        style={{
          flex: '0 0 260px',
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          fontWeight: 500,
        }}
      >
        {title || <span style={{ color: 'var(--fg-dim, #6b7280)' }}>Untitled</span>}
      </span>
      <span
        style={{
          flex: 1,
          display: 'flex',
          gap: 12,
          color: 'var(--fg-muted, #9ca3af)',
          overflow: 'hidden',
        }}
      >
        {detail.map((c) => {
          const v = record[c.name]
          if (v == null || v === '') return null
          return (
            <span
              key={c.name}
              style={{
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
              }}
            >
              <span style={{ color: 'var(--fg-dim, #6b7280)' }}>{c.name}: </span>
              {formatValue(c.def.type, v)}
            </span>
          )
        })}
      </span>
    </button>
  )
}

function groupKeyFor(col: Column, raw: unknown): string {
  if (raw == null || raw === '') return EMPTY_KEY
  if (col.def.type === 'multi-select' && Array.isArray(raw)) {
    // Group multi-select by the sorted join; matches how it renders
    // in the cell and keeps the same multi-tag combination together.
    const sorted = [...(raw as unknown[])].map(String).sort()
    return sorted.length ? sorted.join(', ') : EMPTY_KEY
  }
  if (col.def.type === 'checkbox') return raw ? 'Checked' : 'Unchecked'
  return String(raw)
}

function groupKeyLabel(col: Column, key: string): string {
  if (col.def.type === 'checkbox') return key
  return key
}
