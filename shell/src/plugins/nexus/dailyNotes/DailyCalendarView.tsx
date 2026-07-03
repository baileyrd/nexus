// C6 (#359) — small month-calendar pane for daily-note navigation.
// Grid-building logic mirrors bases/BasesCalendar.tsx's buildMonthCells
// (42-cell 6×7 grid, week starting Sunday); the shared date helpers
// (isoDate/addMonths/startOfDay/yyyymm/parseYyyymm/labels) are imported
// directly from bases/dateUtils.ts rather than duplicated.

import { useMemo, useState } from 'react'
import { MONTH_LABELS, WEEKDAY_LABELS, addMonths, isoDate, startOfDay } from '../bases/dateUtils'
import { openDailyNote } from './openDailyNote'

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

export function DailyCalendarView() {
  const [month, setMonth] = useState(() => startOfDay(new Date()))
  const cells = useMemo(() => buildMonthCells(month), [month])
  const todayIso = useMemo(() => isoDate(new Date()), [])

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--background-primary)',
        fontFamily: 'var(--font-interface)',
      }}
    >
      <div
        style={{
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '6px 12px',
          borderBottom: '1px solid var(--divider-color)',
        }}
      >
        <button
          type="button"
          onClick={() => setMonth((m) => addMonths(m, -1))}
          style={navButtonStyle}
          aria-label="Previous month"
        >
          ‹
        </button>
        <span style={{ flex: 1, textAlign: 'center', fontSize: 12, fontWeight: 600, color: 'var(--text-normal)' }}>
          {MONTH_LABELS[month.getMonth()]} {month.getFullYear()}
        </span>
        <button
          type="button"
          onClick={() => setMonth((m) => addMonths(m, 1))}
          style={navButtonStyle}
          aria-label="Next month"
        >
          ›
        </button>
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(7, 1fr)', padding: '4px 8px 0' }}>
        {WEEKDAY_LABELS.map((w) => (
          <div
            key={w}
            style={{ textAlign: 'center', fontSize: 10, color: 'var(--text-faint)', padding: 4 }}
          >
            {w}
          </div>
        ))}
      </div>
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(7, 1fr)',
          gridAutoRows: '1fr',
          flex: 1,
          padding: '0 8px 8px',
          gap: 2,
        }}
      >
        {cells.map((cell) => {
          const iso = isoDate(cell.day)
          return (
            <button
              key={iso}
              type="button"
              onClick={() => void openDailyNote(cell.day)}
              title={iso}
              style={{
                border: cell.isToday ? '1px solid var(--interactive-accent)' : '1px solid transparent',
                borderRadius: 4,
                background: iso === todayIso ? 'var(--interactive-accent-soft)' : 'transparent',
                color: cell.inMonth ? 'var(--text-normal)' : 'var(--text-faint)',
                fontSize: 12,
                cursor: 'pointer',
                padding: 4,
                minHeight: 28,
              }}
            >
              {cell.day.getDate()}
            </button>
          )
        })}
      </div>
    </div>
  )
}

const navButtonStyle: React.CSSProperties = {
  background: 'none',
  border: '1px solid var(--divider-color)',
  borderRadius: 4,
  color: 'var(--text-muted)',
  cursor: 'pointer',
  fontSize: 14,
  width: 24,
  height: 24,
  lineHeight: 1,
}
