// Phase 4 of docs/bases-shell-plan.md — Calendar view. Month grid
// keyed on a `date`/`datetime` property. Click an empty cell → new
// record with that date prefilled. Record chips are click-through
// (select the row globally so the Table/List/Board selection stays
// in sync).

import { useMemo, useState } from 'react'
import { useBasesStore } from './basesStore'
import type { Base, BaseRecord, BasesKernelClient } from './kernelClient'
import {
  defaultValueFor,
  formatValue,
  isReadOnly,
  parseFieldDef,
  type FieldDefinition,
} from './fieldTypes'
import {
  MONTH_LABELS,
  WEEKDAY_LABELS,
  addMonths,
  isoDate,
  parseDate,
  parseYyyymm,
  startOfDay,
  yyyymm,
} from './dateUtils'

interface Props {
  relpath: string
  base: Base
  client: BasesKernelClient
}

interface Column {
  name: string
  def: FieldDefinition
}

export function BasesCalendar({ relpath, base, client }: Props) {
  const dateField = useBasesStore((s) => s.tabs[relpath]?.calendarDateField ?? null)
  const monthKey = useBasesStore((s) => s.tabs[relpath]?.calendarMonth ?? null)
  const setDateField = useBasesStore((s) => s.setCalendarDateField)
  const setMonth = useBasesStore((s) => s.setCalendarMonth)
  const appendRecord = useBasesStore((s) => s.appendRecord)
  const setSelectedRecordId = useBasesStore((s) => s.setSelectedRecordId)

  const [opError, setOpError] = useState<string | null>(null)

  const dateColumns = useMemo(() => dateFields(base), [base])
  const allColumns = useMemo(() => allFields(base), [base])
  const primary = useMemo(
    () => allColumns.find((c) => c.def.primary) ?? allColumns[0],
    [allColumns],
  )

  const active = useMemo<Column | null>(() => {
    if (dateColumns.length === 0) return null
    if (dateField) {
      const m = dateColumns.find((c) => c.name === dateField)
      if (m) return m
    }
    return dateColumns[0]
  }, [dateField, dateColumns])

  const monthStart = useMemo(() => parseYyyymm(monthKey), [monthKey])
  const cells = useMemo(() => buildMonthCells(monthStart), [monthStart])

  const recordsByDay = useMemo(() => {
    const map = new Map<string, BaseRecord[]>()
    if (!active) return map
    for (const r of base.records) {
      const d = parseDate(r[active.name])
      if (!d) continue
      const key = isoDate(d)
      const existing = map.get(key)
      if (existing) existing.push(r)
      else map.set(key, [r])
    }
    return map
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
          padding: 24,
        }}
      >
        The Calendar view needs at least one <code style={{ margin: '0 4px' }}>date</code> or <code style={{ margin: '0 4px' }}>datetime</code> field.
      </div>
    )
  }

  const createOnDay = async (day: Date) => {
    try {
      setOpError(null)
      const seed: Record<string, unknown> = { [active.name]: isoDate(day) }
      for (const { name, def } of allColumns) {
        if (name === active.name) continue
        if (def.required && !isReadOnly(def.type)) {
          seed[name] = defaultValueFor(def.type)
        }
      }
      const stored = await client.createRecord(relpath, { id: '', ...seed } as BaseRecord)
      appendRecord(relpath, stored)
      setSelectedRecordId(relpath, stored.id)
    } catch (err) {
      setOpError(`create failed: ${errMsg(err)}`)
    }
  }

  const title = `${MONTH_LABELS[monthStart.getMonth()]} ${monthStart.getFullYear()}`

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
        <button
          type="button"
          onClick={() => setMonth(relpath, yyyymm(addMonths(monthStart, -1)))}
          style={toolbarBtnStyle}
        >
          ‹
        </button>
        <strong style={{ color: 'var(--fg-primary, #e4e4e7)' }}>{title}</strong>
        <button
          type="button"
          onClick={() => setMonth(relpath, yyyymm(addMonths(monthStart, 1)))}
          style={toolbarBtnStyle}
        >
          ›
        </button>
        <button
          type="button"
          onClick={() => setMonth(relpath, null)}
          style={toolbarBtnStyle}
        >
          Today
        </button>
        <span>·</span>
        <span>Date field</span>
        <select
          value={active.name}
          onChange={(e) => setDateField(relpath, e.target.value || null)}
          style={selectStyle}
        >
          {dateColumns.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        {opError && <span style={{ color: 'var(--risk, #f48771)' }}>{opError}</span>}
      </div>
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(7, 1fr)',
          borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
          background: 'var(--bg-raised, #252529)',
          color: 'var(--fg-muted, #9ca3af)',
          fontSize: 11,
        }}
      >
        {WEEKDAY_LABELS.map((label) => (
          <div key={label} style={{ padding: '6px 8px', textAlign: 'center' }}>
            {label}
          </div>
        ))}
      </div>
      <div
        style={{
          flex: 1,
          display: 'grid',
          gridTemplateColumns: 'repeat(7, 1fr)',
          gridAutoRows: '1fr',
          overflow: 'auto',
        }}
      >
        {cells.map((cell) => {
          const key = isoDate(cell.day)
          const recs = recordsByDay.get(key) ?? []
          return (
            <div
              key={key}
              onClick={() => void createOnDay(cell.day)}
              style={{
                borderRight: '1px solid var(--border-subtle, #2a2a2e)',
                borderBottom: '1px solid var(--border-subtle, #2a2a2e)',
                padding: 4,
                minHeight: 72,
                display: 'flex',
                flexDirection: 'column',
                gap: 2,
                background: cell.inMonth
                  ? 'var(--bg-primary, #1e1e1e)'
                  : 'var(--bg-raised-dim, #1a1a1d)',
                color: cell.inMonth
                  ? 'var(--fg-primary, #e4e4e7)'
                  : 'var(--fg-dim, #6b7280)',
                cursor: 'pointer',
                fontSize: 11,
              }}
              title="Click to add"
            >
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                }}
              >
                <span
                  style={{
                    fontWeight: cell.isToday ? 600 : 400,
                    color: cell.isToday
                      ? 'var(--accent, #60a5fa)'
                      : cell.inMonth
                        ? 'var(--fg-primary, #e4e4e7)'
                        : 'var(--fg-dim, #6b7280)',
                  }}
                >
                  {cell.day.getDate()}
                </span>
              </div>
              {recs.slice(0, 3).map((r) => {
                const label = primary
                  ? formatValue(primary.def.type, r[primary.name]) || 'Untitled'
                  : r.id
                return (
                  <button
                    key={r.id}
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation()
                      setSelectedRecordId(relpath, r.id)
                    }}
                    style={{
                      textAlign: 'left',
                      background: 'var(--bg-raised, #252529)',
                      border: '1px solid var(--border-subtle, #2a2a2e)',
                      borderRadius: 3,
                      padding: '1px 4px',
                      fontSize: 10,
                      color: 'var(--fg-primary, #e4e4e7)',
                      whiteSpace: 'nowrap',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      cursor: 'pointer',
                    }}
                    title={label}
                  >
                    {label}
                  </button>
                )
              })}
              {recs.length > 3 && (
                <span style={{ fontSize: 10, color: 'var(--fg-dim, #6b7280)' }}>
                  +{recs.length - 3} more
                </span>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

interface Cell {
  day: Date
  inMonth: boolean
  isToday: boolean
}

function buildMonthCells(monthStart: Date): Cell[] {
  const first = new Date(monthStart.getFullYear(), monthStart.getMonth(), 1)
  const gridStart = new Date(first)
  gridStart.setDate(first.getDate() - first.getDay())
  const today = startOfDay(new Date())
  const cells: Cell[] = []
  for (let i = 0; i < 42; i += 1) {
    const d = new Date(gridStart.getFullYear(), gridStart.getMonth(), gridStart.getDate() + i)
    cells.push({
      day: d,
      inMonth: d.getMonth() === monthStart.getMonth(),
      isToday: d.valueOf() === today.valueOf(),
    })
  }
  return cells
}

function dateFields(base: Base): Column[] {
  return Object.entries(base.schema.fields ?? {})
    .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
    .filter((c) => c.def.type === 'date' || c.def.type === 'datetime')
}

function allFields(base: Base): Column[] {
  return Object.entries(base.schema.fields ?? {})
    .filter(([n]) => n !== 'id')
    .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
}

function errMsg(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

const toolbarBtnStyle: React.CSSProperties = {
  padding: '3px 8px',
  background: 'var(--bg-raised, #252529)',
  color: 'var(--fg-primary, #e4e4e7)',
  border: '1px solid var(--border-subtle, #2a2a2e)',
  borderRadius: 3,
  fontSize: 11,
  cursor: 'pointer',
}

const selectStyle: React.CSSProperties = {
  background: 'var(--bg-raised, #252529)',
  color: 'var(--fg-primary, #e4e4e7)',
  border: '1px solid var(--border-subtle, #2a2a2e)',
  borderRadius: 3,
  padding: '2px 6px',
  fontSize: 11,
}
