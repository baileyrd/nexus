import { useMemo } from 'react'
import {
  originKind,
  useActivityTimelineStore,
  type ActivityEntry,
  type ActivityOriginKind,
  type ActivitySurface,
  type IsoDate,
} from './activityTimelineStore'

/**
 * Local-date prefix (`YYYY-MM-DD`) of an entry timestamp. Returns null
 * when the timestamp is unparseable so the date filter degrades to "no
 * match" rather than throwing.
 */
function entryLocalDate(ts: string): IsoDate | null {
  const d = new Date(ts)
  if (Number.isNaN(d.getTime())) return null
  const yyyy = d.getFullYear()
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${yyyy}-${mm}-${dd}`
}

/** Inclusive on both bounds; either side may be null. */
function entryInDateRange(
  e: ActivityEntry,
  from: IsoDate | null,
  to: IsoDate | null,
): boolean {
  if (from === null && to === null) return true
  const day = entryLocalDate(e.timestamp)
  if (day === null) return false
  if (from !== null && day < from) return false
  if (to !== null && day > to) return false
  return true
}

/**
 * BL-037 — pane-mode view for the AI activity timeline.
 *
 *   ┌──────────────────────────────────────────────────────────────┐
 *   │ ACTIVITY TIMELINE   [filter] [surface ▾] [clear]   {n}/{tot} │
 *   ├──────────────────────────────────────────────────────────────┤
 *   │ 14:25:13  chat   anthropic/claude…   "summarize…"            │
 *   │           files: notes/a.md, …                               │
 *   │           tools: read_file ✓ write_file ✓                    │
 *   │ 14:23:55  cmdi   anthropic/claude…   "rephrase this"  ✗       │
 *   │           error: provider rate-limited                       │
 *   └──────────────────────────────────────────────────────────────┘
 *
 * The view is render-only; the plugin's activate hook owns the
 * hydrate-on-open + bus-subscribe lifecycle.
 */

const SURFACE_OPTIONS: ActivitySurface[] = [
  'chat',
  'ask',
  'cmdi',
  'ghost',
  'complete',
  'enrich',
  'file',
  'process',
  'git',
  'workflow',
  'capability',
  'other',
]

/** BL-052 — origin filter chip values, in display order. */
const ORIGIN_OPTIONS: ActivityOriginKind[] = [
  'ai',
  'user',
  'storage',
  'git',
  'terminal',
  'workflow',
  'agent',
  'plugin',
  'capability',
]

// ── Helpers ────────────────────────────────────────────────────────────

function formatTimestamp(ts: string): string {
  const d = new Date(ts)
  if (Number.isNaN(d.getTime())) return ts
  const hh = String(d.getHours()).padStart(2, '0')
  const mm = String(d.getMinutes()).padStart(2, '0')
  const ss = String(d.getSeconds()).padStart(2, '0')
  return `${hh}:${mm}:${ss}`
}

function formatDuration(ms?: number | null): string {
  if (ms == null) return ''
  if (ms < 1000) return `${ms} ms`
  return `${(ms / 1000).toFixed(1)} s`
}

function surfaceColor(s: ActivitySurface): string {
  switch (s) {
    case 'chat':
      return 'var(--interactive-accent)'
    case 'ask':
      return 'var(--cool)'
    case 'cmdi':
      return 'var(--warm)'
    case 'ghost':
      return 'var(--text-muted)'
    case 'complete':
      return 'var(--ok)'
    case 'enrich':
      return 'var(--interactive-accent-soft)'
    case 'file':
      return 'var(--text-muted)'
    case 'process':
      return 'var(--warm)'
    case 'git':
      return 'var(--ok)'
    case 'workflow':
      return 'var(--interactive-accent-soft)'
    case 'capability':
      return 'var(--risk)'
    default:
      return 'var(--text-faint)'
  }
}

/** BL-052 — short label for the origin filter chip. */
function originLabel(o: ActivityOriginKind): string {
  switch (o) {
    case 'ai':
      return 'AI'
    case 'user':
      return 'User'
    case 'plugin':
      return 'Plugin'
    case 'workflow':
      return 'Workflow'
    case 'agent':
      return 'Agent'
    case 'terminal':
      return 'Terminal'
    case 'git':
      return 'Git'
    case 'storage':
      return 'File'
    case 'capability':
      return 'Capability'
  }
}

function outcomeGlyph(o: ActivityEntry['outcome']): string {
  switch (o) {
    case 'ok':
      return '✓'
    case 'error':
      return '✗'
    case 'cancelled':
      return '∅'
    default:
      return '·'
  }
}

function entryMatchesFilter(e: ActivityEntry, needle: string): boolean {
  if (needle.length === 0) return true
  const n = needle.toLowerCase()
  if (e.surface.toLowerCase().includes(n)) return true
  if ((e.origin ?? '').toLowerCase().includes(n)) return true
  if ((e.provider ?? '').toLowerCase().includes(n)) return true
  if ((e.model ?? '').toLowerCase().includes(n)) return true
  if (e.prompt.toLowerCase().includes(n)) return true
  if (e.files?.some((f) => f.toLowerCase().includes(n))) return true
  if (e.tool_calls?.some((t) => t.name.toLowerCase().includes(n))) return true
  return false
}

// ── Sub-components ────────────────────────────────────────────────────

function Toolbar({
  filteredCount,
  sessionOptions,
  filtersActive,
  onClear,
}: {
  filteredCount: number
  sessionOptions: string[]
  filtersActive: boolean
  onClear: () => void
}) {
  const filter = useActivityTimelineStore((s) => s.filter)
  const setFilter = useActivityTimelineStore((s) => s.setFilter)
  const surfaceFilter = useActivityTimelineStore((s) => s.surfaceFilter)
  const setSurfaceFilter = useActivityTimelineStore((s) => s.setSurfaceFilter)
  const originFilter = useActivityTimelineStore((s) => s.originFilter)
  const setOriginFilter = useActivityTimelineStore((s) => s.setOriginFilter)
  const sessionFilter = useActivityTimelineStore((s) => s.sessionFilter)
  const setSessionFilter = useActivityTimelineStore((s) => s.setSessionFilter)
  const dateFrom = useActivityTimelineStore((s) => s.dateFrom)
  const dateTo = useActivityTimelineStore((s) => s.dateTo)
  const setDateRange = useActivityTimelineStore((s) => s.setDateRange)
  const resetFilters = useActivityTimelineStore((s) => s.resetFilters)
  const total = useActivityTimelineStore((s) => s.entries.length)

  const inputBaseStyle = {
    height: 24,
    background: 'var(--background-secondary)',
    color: 'var(--text-normal)',
    border: '1px solid var(--divider-color)',
    borderRadius: 4,
    fontSize: 11,
    fontFamily: 'var(--font-interface)',
    padding: '0 6px',
  } as const

  return (
    <div
      style={{
        flexShrink: 0,
        borderBottom: '1px solid var(--divider-color)',
        display: 'flex',
        flexWrap: 'wrap',
        alignItems: 'center',
        padding: '6px 12px',
        gap: 8,
        background: 'var(--background-primary)',
      }}
    >
      <input
        type="search"
        placeholder="Filter timeline…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        style={{
          flex: 1,
          minWidth: 160,
          maxWidth: 320,
          height: 24,
          padding: '0 8px',
          background: 'var(--background-secondary)',
          color: 'var(--text-normal)',
          border: '1px solid var(--divider-color)',
          borderRadius: 4,
          fontSize: 12,
          fontFamily: 'var(--font-interface)',
          outline: 'none',
        }}
      />
      <select
        value={originFilter ?? ''}
        onChange={(e) => {
          const v = e.target.value
          setOriginFilter(v === '' ? null : (v as ActivityOriginKind))
        }}
        style={inputBaseStyle}
        title="Filter by origin (BL-052)"
      >
        <option value="">all origins</option>
        {ORIGIN_OPTIONS.map((o) => (
          <option key={o} value={o}>
            {originLabel(o)}
          </option>
        ))}
      </select>
      <select
        value={surfaceFilter ?? ''}
        onChange={(e) => {
          const v = e.target.value
          setSurfaceFilter(v === '' ? null : (v as ActivitySurface))
        }}
        style={inputBaseStyle}
        title="Filter by surface"
      >
        <option value="">all surfaces</option>
        {SURFACE_OPTIONS.map((s) => (
          <option key={s} value={s}>
            {s}
          </option>
        ))}
      </select>
      <select
        value={sessionFilter ?? ''}
        onChange={(e) => {
          const v = e.target.value
          setSessionFilter(v === '' ? null : v)
        }}
        style={{ ...inputBaseStyle, maxWidth: 160 }}
        title="Filter by session id"
        disabled={sessionOptions.length === 0}
      >
        <option value="">all sessions</option>
        {sessionOptions.map((id) => (
          <option key={id} value={id}>
            {id.length > 18 ? `${id.slice(0, 8)}…${id.slice(-6)}` : id}
          </option>
        ))}
      </select>
      <input
        type="date"
        value={dateFrom ?? ''}
        max={dateTo ?? undefined}
        onChange={(e) => setDateRange(e.target.value || null, dateTo)}
        style={inputBaseStyle}
        title="From date (inclusive)"
      />
      <input
        type="date"
        value={dateTo ?? ''}
        min={dateFrom ?? undefined}
        onChange={(e) => setDateRange(dateFrom, e.target.value || null)}
        style={inputBaseStyle}
        title="To date (inclusive)"
      />
      {filtersActive && (
        <button
          onClick={resetFilters}
          title="Clear all filters"
          style={{
            ...inputBaseStyle,
            background: 'transparent',
            color: 'var(--text-muted)',
            cursor: 'pointer',
          }}
        >
          Reset
        </button>
      )}
      <button
        onClick={onClear}
        title="Clear timeline (deletes the on-disk log)"
        style={{
          ...inputBaseStyle,
          background: 'transparent',
          color: 'var(--text-muted)',
          cursor: 'pointer',
        }}
      >
        Clear
      </button>
      <div
        style={{
          marginLeft: 'auto',
          color: 'var(--text-faint)',
          fontFamily: 'var(--font-monospace)',
          fontSize: 11,
        }}
      >
        {filtersActive ? `${filteredCount} / ${total}` : `${total}`}
      </div>
    </div>
  )
}

function EntryRow({ entry }: { entry: ActivityEntry }) {
  const providerLabel =
    entry.provider && entry.model
      ? `${entry.provider}/${entry.model}`
      : (entry.provider ?? entry.model ?? '')
  const outcomeColor =
    entry.outcome === 'error'
      ? 'var(--risk)'
      : entry.outcome === 'cancelled'
        ? 'var(--text-faint)'
        : 'var(--ok)'

  return (
    <div
      style={{
        padding: '8px 14px',
        borderBottom: '1px solid var(--divider-color)',
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
        lineHeight: 1.45,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'baseline',
          gap: 10,
        }}
      >
        <span
          style={{
            color: 'var(--text-faint)',
            fontFamily: 'var(--font-monospace)',
            fontSize: 11,
            flexShrink: 0,
          }}
          title={entry.timestamp}
        >
          {formatTimestamp(entry.timestamp)}
        </span>
        <span
          style={{
            color: surfaceColor(entry.surface),
            fontFamily: 'var(--font-monospace)',
            fontSize: 11,
            textTransform: 'lowercase',
            minWidth: 56,
            flexShrink: 0,
          }}
        >
          {entry.surface}
        </span>
        <span
          style={{
            color: outcomeColor,
            fontFamily: 'var(--font-monospace)',
            fontSize: 12,
            width: 12,
            textAlign: 'center',
            flexShrink: 0,
          }}
          title={entry.outcome}
        >
          {outcomeGlyph(entry.outcome)}
        </span>
        <span
          style={{
            color: 'var(--text-normal)',
            flex: 1,
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {entry.prompt || <em style={{ color: 'var(--text-faint)' }}>(no prompt)</em>}
        </span>
        {entry.duration_ms != null && (
          <span
            style={{
              color: 'var(--text-faint)',
              fontFamily: 'var(--font-monospace)',
              fontSize: 10,
              flexShrink: 0,
            }}
          >
            {formatDuration(entry.duration_ms)}
          </span>
        )}
      </div>
      {(providerLabel ||
        (entry.files && entry.files.length > 0) ||
        (entry.tool_calls && entry.tool_calls.length > 0) ||
        entry.error) && (
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            gap: 12,
            marginTop: 4,
            paddingLeft: 90,
            color: 'var(--text-muted)',
            fontFamily: 'var(--font-monospace)',
            fontSize: 10,
          }}
        >
          {providerLabel && (
            <span title="provider/model">
              <span style={{ color: 'var(--text-faint)' }}>model </span>
              {providerLabel}
            </span>
          )}
          {entry.files && entry.files.length > 0 && (
            <span title={entry.files.join(', ')}>
              <span style={{ color: 'var(--text-faint)' }}>files </span>
              {entry.files.length === 1
                ? entry.files[0]
                : `${entry.files[0]} +${entry.files.length - 1}`}
            </span>
          )}
          {entry.tool_calls && entry.tool_calls.length > 0 && (
            <span>
              <span style={{ color: 'var(--text-faint)' }}>tools </span>
              {entry.tool_calls.map((t, i) => (
                <span key={i} style={{ marginRight: 6 }}>
                  {t.name} {t.ok ? '✓' : '✗'}
                </span>
              ))}
            </span>
          )}
          {entry.error && (
            <span style={{ color: 'var(--risk)' }} title={entry.error}>
              {entry.error.length > 80 ? entry.error.slice(0, 79) + '…' : entry.error}
            </span>
          )}
        </div>
      )}
    </div>
  )
}

function EntryList() {
  const entries = useActivityTimelineStore((s) => s.entries)
  const filter = useActivityTimelineStore((s) => s.filter)
  const surfaceFilter = useActivityTimelineStore((s) => s.surfaceFilter)
  const originFilter = useActivityTimelineStore((s) => s.originFilter)
  const sessionFilter = useActivityTimelineStore((s) => s.sessionFilter)
  const dateFrom = useActivityTimelineStore((s) => s.dateFrom)
  const dateTo = useActivityTimelineStore((s) => s.dateTo)
  const hydrated = useActivityTimelineStore((s) => s.hydrated)

  const filtered = useMemo(() => {
    return entries.filter(
      (e) =>
        (surfaceFilter === null || e.surface === surfaceFilter) &&
        (originFilter === null || originKind(e.origin ?? 'ai') === originFilter) &&
        (sessionFilter === null || e.session_id === sessionFilter) &&
        entryInDateRange(e, dateFrom, dateTo) &&
        entryMatchesFilter(e, filter.trim()),
    )
  }, [entries, filter, surfaceFilter, originFilter, sessionFilter, dateFrom, dateTo])

  if (!hydrated) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--text-faint)',
          fontSize: 12,
        }}
      >
        Loading activity…
      </div>
    )
  }

  if (entries.length === 0) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 8,
          color: 'var(--text-faint)',
          fontSize: 12,
          padding: 24,
          textAlign: 'center',
          lineHeight: 1.6,
        }}
      >
        <div>No activity yet.</div>
        <div>
          Every AI call, file write, git commit, terminal session, and
          workflow run is recorded here — prompt, model, files touched,
          tools, and outcome. Use the origin filter to slice by source.
        </div>
        <div style={{ marginTop: 6 }}>
          <a
            href="https://github.com/nexus-app/nexus/blob/main/docs/PRDs/12-ai-engine.md"
            target="_blank"
            rel="noreferrer noopener"
            style={{
              color: 'var(--text-muted)',
              textDecoration: 'underline',
            }}
          >
            What gets recorded?
          </a>
        </div>
      </div>
    )
  }

  if (filtered.length === 0) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--text-faint)',
          fontSize: 12,
        }}
      >
        No entries match the filter.
      </div>
    )
  }

  return (
    <div
      style={{
        flex: 1,
        overflowY: 'auto',
        background: 'var(--background-primary)',
      }}
    >
      {filtered.map((e) => (
        <EntryRow key={e.id} entry={e} />
      ))}
    </div>
  )
}

// ── Root ────────────────────────────────────────────────────────────────

export function ActivityTimelineView({
  onClear,
}: {
  onClear: () => void
}) {
  const entries = useActivityTimelineStore((s) => s.entries)
  const filter = useActivityTimelineStore((s) => s.filter)
  const surfaceFilter = useActivityTimelineStore((s) => s.surfaceFilter)
  const originFilter = useActivityTimelineStore((s) => s.originFilter)
  const sessionFilter = useActivityTimelineStore((s) => s.sessionFilter)
  const dateFrom = useActivityTimelineStore((s) => s.dateFrom)
  const dateTo = useActivityTimelineStore((s) => s.dateTo)

  const sessionOptions = useMemo(() => {
    const seen = new Set<string>()
    const ordered: string[] = []
    for (const e of entries) {
      if (e.session_id && !seen.has(e.session_id)) {
        seen.add(e.session_id)
        ordered.push(e.session_id)
      }
    }
    return ordered
  }, [entries])

  const filtersActive =
    filter.length > 0 ||
    surfaceFilter !== null ||
    originFilter !== null ||
    sessionFilter !== null ||
    dateFrom !== null ||
    dateTo !== null

  const filteredCount = useMemo(() => {
    return entries.reduce(
      (n, e) =>
        (surfaceFilter === null || e.surface === surfaceFilter) &&
        (originFilter === null || originKind(e.origin ?? 'ai') === originFilter) &&
        (sessionFilter === null || e.session_id === sessionFilter) &&
        entryInDateRange(e, dateFrom, dateTo) &&
        entryMatchesFilter(e, filter.trim())
          ? n + 1
          : n,
      0,
    )
  }, [entries, filter, surfaceFilter, originFilter, sessionFilter, dateFrom, dateTo])

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        width: '100%',
        background: 'var(--background-primary)',
        color: 'var(--text-normal)',
        fontFamily: 'var(--font-interface)',
      }}
    >
      <div
        style={{
          padding: '10px 14px',
          fontSize: 11,
          color: 'var(--text-muted)',
          textTransform: 'uppercase',
          letterSpacing: 1,
          borderBottom: '1px solid var(--divider-color)',
        }}
      >
        Activity Timeline
      </div>
      <Toolbar
        filteredCount={filteredCount}
        sessionOptions={sessionOptions}
        filtersActive={filtersActive}
        onClear={onClear}
      />
      <EntryList />
    </div>
  )
}
