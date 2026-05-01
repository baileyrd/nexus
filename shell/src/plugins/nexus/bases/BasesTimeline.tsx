// Phase 4 of docs/bases-shell-plan.md — Timeline view. Swimlanes
// keyed on a `select`; each record renders as a horizontal bar
// spanning start→end dates. Zoomable via a day-px slider. Scope
// for Phase 4: read-only rendering, click-to-select; resize + drag
// reschedule lands in Phase 6.

import { useMemo } from 'react'
import { useBasesStore } from './basesStore'
import type { Base, BaseRecord, BasesKernelClient } from './kernelClient'
import { formatValue, parseFieldDef, type FieldDefinition } from './fieldTypes'
import {
  MONTH_LABELS,
  addDays,
  daysBetween,
  isoDate,
  parseDate,
  startOfDay,
} from './dateUtils'

interface Props {
  relpath: string
  base: Base
  /** Reserved — Phase 6 wires drag/resize reschedule through this. */
  client: BasesKernelClient
}

interface Column {
  name: string
  def: FieldDefinition
}

const UNASSIGNED = '__unassigned__'

export function BasesTimeline({ relpath, base, client: _client }: Props) {
  const groupField = useBasesStore((s) => s.tabs[relpath]?.timelineGroupField ?? null)
  const startField = useBasesStore((s) => s.tabs[relpath]?.timelineStartField ?? null)
  const endField = useBasesStore((s) => s.tabs[relpath]?.timelineEndField ?? null)
  const dayPx = useBasesStore((s) => s.tabs[relpath]?.timelineDayPx ?? 24)
  const setGroup = useBasesStore((s) => s.setTimelineGroupField)
  const setStart = useBasesStore((s) => s.setTimelineStartField)
  const setEnd = useBasesStore((s) => s.setTimelineEndField)
  const setDayPx = useBasesStore((s) => s.setTimelineDayPx)
  const setSelectedRecordId = useBasesStore((s) => s.setSelectedRecordId)

  const selectColumns = useMemo(() => columnsOfType(base, 'select'), [base])
  const dateColumns = useMemo(
    () =>
      Object.entries(base.schema.fields ?? {})
        .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
        .filter((c) => c.def.type === 'date' || c.def.type === 'datetime'),
    [base],
  )
  const allColumns = useMemo(
    () =>
      Object.entries(base.schema.fields ?? {})
        .filter(([n]) => n !== 'id')
        .map(([name, def]) => ({ name, def: parseFieldDef(def) })),
    [base],
  )
  const primary = useMemo(
    () => allColumns.find((c) => c.def.primary) ?? allColumns[0],
    [allColumns],
  )

  const activeStart = resolveColumn(dateColumns, startField) ?? dateColumns[0] ?? null
  const activeEnd =
    resolveColumn(dateColumns, endField) ??
    (dateColumns.length > 1 ? dateColumns[1] : null) ??
    activeStart
  const activeGroup = resolveColumn(selectColumns, groupField) ?? selectColumns[0] ?? null

  const layout = useMemo(() => {
    if (!activeStart || !activeEnd) return null
    const events: Array<{
      record: BaseRecord
      start: Date
      end: Date
      group: string
    }> = []
    let minDay: Date | null = null
    let maxDay: Date | null = null
    for (const r of base.records) {
      const s = parseDate(r[activeStart.name])
      if (!s) continue
      const eRaw = parseDate(r[activeEnd.name]) ?? s
      const start = startOfDay(s)
      const end = startOfDay(eRaw.valueOf() < s.valueOf() ? s : eRaw)
      const groupRaw = activeGroup ? r[activeGroup.name] : null
      const group = typeof groupRaw === 'string' && groupRaw ? groupRaw : UNASSIGNED
      events.push({ record: r, start, end, group })
      if (!minDay || start.valueOf() < minDay.valueOf()) minDay = start
      if (!maxDay || end.valueOf() > maxDay.valueOf()) maxDay = end
    }
    if (!minDay || !maxDay) return null
    const paddedStart = addDays(minDay, -2)
    const paddedEnd = addDays(maxDay, 4)
    const totalDays = Math.max(1, daysBetween(paddedStart, paddedEnd) + 1)
    // Group ordering: declared `select.options` first, then observed
    // values not in options, then Unassigned last.
    const groupOrder: string[] = []
    const seen = new Set<string>()
    if (activeGroup) {
      for (const opt of activeGroup.def.options ?? []) {
        groupOrder.push(opt)
        seen.add(opt)
      }
    }
    for (const ev of events) {
      if (!seen.has(ev.group) && ev.group !== UNASSIGNED) {
        groupOrder.push(ev.group)
        seen.add(ev.group)
      }
    }
    if (events.some((e) => e.group === UNASSIGNED) || groupOrder.length === 0) {
      groupOrder.push(UNASSIGNED)
    }
    const byGroup = new Map<string, typeof events>()
    for (const g of groupOrder) byGroup.set(g, [])
    for (const ev of events) {
      const bucket = byGroup.get(ev.group) ?? byGroup.get(UNASSIGNED)
      if (bucket) bucket.push(ev)
    }
    return { paddedStart, totalDays, groupOrder, byGroup }
  }, [activeStart, activeEnd, activeGroup, base.records])

  if (!activeStart) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--text-muted, #9ca3af)',
          fontSize: 12,
          padding: 24,
        }}
      >
        The Timeline view needs at least one <code style={{ margin: '0 4px' }}>date</code> field for start.
      </div>
    )
  }

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: 8,
          padding: '6px 12px',
          borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
          fontSize: 12,
          color: 'var(--text-muted, #9ca3af)',
        }}
      >
        <span>Start</span>
        <select
          value={activeStart.name}
          onChange={(e) => setStart(relpath, e.target.value || null)}
          style={selectStyle}
        >
          {dateColumns.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        <span>End</span>
        <select
          value={activeEnd?.name ?? activeStart.name}
          onChange={(e) => setEnd(relpath, e.target.value || null)}
          style={selectStyle}
        >
          {dateColumns.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        <span>Group</span>
        <select
          value={activeGroup?.name ?? ''}
          onChange={(e) => setGroup(relpath, e.target.value || null)}
          style={selectStyle}
          disabled={selectColumns.length === 0}
        >
          <option value="">(none)</option>
          {selectColumns.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        <span style={{ marginLeft: 12 }}>Zoom</span>
        <input
          type="range"
          min={4}
          max={60}
          step={2}
          value={dayPx}
          onChange={(e) => setDayPx(relpath, Number(e.target.value))}
        />
        <span>{dayPx}px/day</span>
      </div>
      {layout ? (
        <TimelineBody
          relpath={relpath}
          paddedStart={layout.paddedStart}
          totalDays={layout.totalDays}
          groupOrder={layout.groupOrder}
          byGroup={layout.byGroup}
          dayPx={dayPx}
          primary={primary}
          onSelect={(id) => setSelectedRecordId(relpath, id)}
        />
      ) : (
        <div
          style={{
            flex: 1,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            color: 'var(--text-muted, #9ca3af)',
            fontSize: 12,
          }}
        >
          No records with a valid start date yet.
        </div>
      )}
    </div>
  )
}

interface BodyProps {
  relpath: string
  paddedStart: Date
  totalDays: number
  groupOrder: string[]
  byGroup: Map<
    string,
    Array<{ record: BaseRecord; start: Date; end: Date; group: string }>
  >
  dayPx: number
  primary: Column | undefined
  onSelect(recordId: string): void
}

const LANE_HEIGHT = 32
const GROUP_LABEL_WIDTH = 140

function TimelineBody({
  paddedStart,
  totalDays,
  groupOrder,
  byGroup,
  dayPx,
  primary,
  onSelect,
}: BodyProps) {
  const contentWidth = totalDays * dayPx
  const months = useMemo(() => monthTicks(paddedStart, totalDays), [paddedStart, totalDays])
  const today = startOfDay(new Date())
  const todayOffset = daysBetween(paddedStart, today)
  const showToday = todayOffset >= 0 && todayOffset <= totalDays

  return (
    <div style={{ flex: 1, display: 'flex', overflow: 'auto' }}>
      <div
        style={{
          flex: '0 0 auto',
          width: GROUP_LABEL_WIDTH,
          borderRight: '1px solid var(--background-modifier-border, #2a2a2e)',
          background: 'var(--background-secondary, #252529)',
          position: 'sticky',
          left: 0,
          zIndex: 2,
        }}
      >
        <div
          style={{
            height: 32,
            borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
          }}
        />
        {groupOrder.map((g) => (
          <div
            key={g}
            style={{
              height: LANE_HEIGHT,
              display: 'flex',
              alignItems: 'center',
              padding: '0 10px',
              borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
              fontSize: 11,
              color:
                g === UNASSIGNED ? 'var(--text-faint, #6b7280)' : 'var(--text-normal, #e4e4e7)',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {g === UNASSIGNED ? 'Unassigned' : g}
          </div>
        ))}
      </div>
      <div style={{ position: 'relative', width: contentWidth, flex: '0 0 auto' }}>
        <div
          style={{
            height: 32,
            borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
            background: 'var(--background-secondary, #252529)',
            position: 'sticky',
            top: 0,
            zIndex: 1,
            color: 'var(--text-muted, #9ca3af)',
            fontSize: 11,
          }}
        >
          {months.map((m) => (
            <div
              key={m.key}
              style={{
                position: 'absolute',
                top: 0,
                left: m.offsetDays * dayPx,
                width: m.days * dayPx,
                height: '100%',
                display: 'flex',
                alignItems: 'center',
                padding: '0 6px',
                borderRight: '1px solid var(--background-modifier-border, #2a2a2e)',
                boxSizing: 'border-box',
              }}
            >
              {m.label}
            </div>
          ))}
        </div>
        <div style={{ position: 'relative' }}>
          {showToday && (
            <div
              style={{
                position: 'absolute',
                top: 0,
                bottom: 0,
                left: todayOffset * dayPx,
                width: 1,
                background: 'var(--interactive-accent, #60a5fa)',
                opacity: 0.6,
                pointerEvents: 'none',
              }}
              title={`Today · ${isoDate(today)}`}
            />
          )}
          {groupOrder.map((g) => {
            const events = byGroup.get(g) ?? []
            return (
              <div
                key={g}
                style={{
                  position: 'relative',
                  height: LANE_HEIGHT,
                  borderBottom: '1px solid var(--background-modifier-border, #2a2a2e)',
                }}
              >
                {events.map((ev) => {
                  const offset = daysBetween(paddedStart, ev.start)
                  const span = Math.max(1, daysBetween(ev.start, ev.end) + 1)
                  const label = primary
                    ? formatValue(primary.def.type, ev.record[primary.name]) || 'Untitled'
                    : ev.record.id
                  return (
                    <button
                      key={ev.record.id}
                      type="button"
                      onClick={() => onSelect(ev.record.id)}
                      style={{
                        position: 'absolute',
                        left: offset * dayPx,
                        top: 4,
                        width: span * dayPx - 2,
                        height: LANE_HEIGHT - 8,
                        background: 'var(--interactive-accent, #60a5fa)',
                        color: 'var(--interactive-accent-ink, #0f1117)',
                        border: 'none',
                        borderRadius: 3,
                        padding: '0 6px',
                        fontSize: 11,
                        whiteSpace: 'nowrap',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        cursor: 'pointer',
                        textAlign: 'left',
                      }}
                      title={`${label} · ${isoDate(ev.start)} → ${isoDate(ev.end)}`}
                    >
                      {label}
                    </button>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

interface MonthTick {
  key: string
  label: string
  offsetDays: number
  days: number
}

function monthTicks(paddedStart: Date, totalDays: number): MonthTick[] {
  const ticks: MonthTick[] = []
  let cursor = new Date(paddedStart.getFullYear(), paddedStart.getMonth(), 1)
  if (cursor.valueOf() < paddedStart.valueOf()) {
    cursor = new Date(paddedStart.getFullYear(), paddedStart.getMonth() + 1, 1)
  }
  // First tick covers from paddedStart to the next month boundary.
  const firstBoundary = new Date(paddedStart.getFullYear(), paddedStart.getMonth() + 1, 1)
  ticks.push({
    key: `${paddedStart.getFullYear()}-${paddedStart.getMonth()}`,
    label: `${MONTH_LABELS[paddedStart.getMonth()]} ${paddedStart.getFullYear()}`,
    offsetDays: 0,
    days: Math.min(daysBetween(paddedStart, firstBoundary), totalDays),
  })
  let offset = ticks[0].days
  cursor = firstBoundary
  while (offset < totalDays) {
    const nextBoundary = new Date(cursor.getFullYear(), cursor.getMonth() + 1, 1)
    const span = Math.min(daysBetween(cursor, nextBoundary), totalDays - offset)
    if (span <= 0) break
    ticks.push({
      key: `${cursor.getFullYear()}-${cursor.getMonth()}`,
      label: `${MONTH_LABELS[cursor.getMonth()]} ${cursor.getFullYear()}`,
      offsetDays: offset,
      days: span,
    })
    offset += span
    cursor = nextBoundary
  }
  return ticks
}

function columnsOfType(base: Base, kind: string): Column[] {
  return Object.entries(base.schema.fields ?? {})
    .map(([name, def]) => ({ name, def: parseFieldDef(def) }))
    .filter((c) => c.def.type === kind)
}

function resolveColumn(cols: Column[], name: string | null): Column | null {
  if (!name) return null
  return cols.find((c) => c.name === name) ?? null
}

const selectStyle: React.CSSProperties = {
  background: 'var(--background-secondary, #252529)',
  color: 'var(--text-normal, #e4e4e7)',
  border: '1px solid var(--background-modifier-border, #2a2a2e)',
  borderRadius: 3,
  padding: '2px 6px',
  fontSize: 11,
}
