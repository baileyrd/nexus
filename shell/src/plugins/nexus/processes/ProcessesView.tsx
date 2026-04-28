import { useEffect, useMemo, useRef } from 'react'
import {
  useProcessesStore,
  PROCESS_EVENTS_CAP,
  type PluginItem,
  type SessionItem,
} from './processesStore'

/**
 * Pane-mode view for `nexus.processes`. Two columns:
 *
 *   ┌─────────────────┬──────────────────────────────────────────────┐
 *   │ PROCESSES       │ [filter] [follow] [clear]       {n}/{CAP}    │
 *   │ ┌─ Plugins — N  │ ───────────────────────────────────────────  │
 *   │ │  (rows)       │ 14:25:13.441  com.nexus.storage.file_modified │
 *   │ └─ Sessions — N │                {"path":"notes/…"}             │
 *   │    (rows)       │ …                                             │
 *   └─────────────────┴──────────────────────────────────────────────┘
 *
 * Everything reads from `useProcessesStore`. The plugin's activate hook
 * is responsible for feeding plugins / sessions / events; this file
 * renders only.
 */

// ── Small helpers ──────────────────────────────────────────────────────

function formatTs(ms: number): string {
  const d = new Date(ms)
  const hh = String(d.getHours()).padStart(2, '0')
  const mm = String(d.getMinutes()).padStart(2, '0')
  const ss = String(d.getSeconds()).padStart(2, '0')
  const msStr = String(d.getMilliseconds()).padStart(3, '0')
  return `${hh}:${mm}:${ss}.${msStr}`
}

function truncate(s: string, n: number): string {
  if (s.length <= n) return s
  return s.slice(0, n - 1) + '…'
}

/** Color for the status dot based on a free-form state label. */
function dotColorForState(state: string): string {
  const s = state.toLowerCase()
  if (s === 'active' || s === 'enabled' || s === 'running' || s === 'connected') {
    return 'var(--ok)'
  }
  if (s === 'error' || s === 'failed' || s === 'crashed') {
    return 'var(--risk)'
  }
  if (s === 'inactive' || s === 'stopped' || s === 'disabled') {
    return 'var(--fg-dim)'
  }
  return 'var(--cool)'
}

function kindGlyph(kind: SessionItem['kind']): string {
  switch (kind) {
    case 'terminal':
      return '▸'
    case 'mcp':
      return '◈'
    default:
      return '•'
  }
}

// ── Left column rows ───────────────────────────────────────────────────

function PluginRow({ p }: { p: PluginItem }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '4px 14px',
        fontSize: 12,
        lineHeight: 1.4,
        color: 'var(--fg)',
        cursor: 'default',
      }}
      title={p.error ? `${p.id}\n${p.error}` : p.id}
    >
      <span
        style={{
          width: 6,
          height: 6,
          borderRadius: '50%',
          background: dotColorForState(p.state),
          flexShrink: 0,
        }}
      />
      <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {p.name}
      </span>
      <span style={{ color: 'var(--fg-dim)', fontFamily: 'var(--f-mono)', fontSize: 10 }}>
        {p.version}
      </span>
    </div>
  )
}

function SessionRow({ s }: { s: SessionItem }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '4px 14px',
        fontSize: 12,
        lineHeight: 1.4,
        color: 'var(--fg)',
        cursor: 'default',
      }}
      title={`${s.kind}: ${s.id}`}
    >
      <span style={{ width: 10, color: 'var(--fg-dim)', flexShrink: 0, textAlign: 'center' }}>
        {kindGlyph(s.kind)}
      </span>
      <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {s.label}
      </span>
      {s.detail && (
        <span style={{ color: 'var(--fg-dim)', fontFamily: 'var(--f-mono)', fontSize: 10 }}>
          {s.detail}
        </span>
      )}
    </div>
  )
}

// ── Sections ────────────────────────────────────────────────────────────

function SectionHead({ title, count }: { title: string; count: number }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '8px 14px 4px',
        color: 'var(--fg-muted)',
        fontSize: 11,
        fontFamily: 'var(--f-ui)',
        textTransform: 'uppercase',
        letterSpacing: 0.5,
      }}
    >
      <span>{title}</span>
      <span style={{ fontFamily: 'var(--f-mono)', fontSize: 10 }}>{count}</span>
    </div>
  )
}

function LeftColumn() {
  const plugins = useProcessesStore((s) => s.plugins)
  const sessions = useProcessesStore((s) => s.sessions)

  return (
    <div
      style={{
        width: 280,
        flexShrink: 0,
        borderRight: '1px solid var(--line-soft)',
        background: 'var(--bg-raised)',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          padding: '10px 14px',
          fontSize: 11,
          color: 'var(--fg-muted)',
          textTransform: 'uppercase',
          letterSpacing: 1,
          borderBottom: '1px solid var(--line-soft)',
        }}
      >
        Processes
      </div>

      <div style={{ flex: 1, overflowY: 'auto' }}>
        <SectionHead title={`Plugins — ${plugins.length}`} count={plugins.length} />
        {plugins.length === 0 ? (
          <div
            style={{
              padding: '4px 14px',
              fontSize: 11,
              color: 'var(--fg-dim)',
              fontStyle: 'italic',
            }}
          >
            No plugins loaded.
          </div>
        ) : (
          plugins.map((p) => <PluginRow key={`${p.source}:${p.id}`} p={p} />)
        )}

        <SectionHead title={`Sessions — ${sessions.length}`} count={sessions.length} />
        {sessions.length === 0 ? (
          <div
            style={{
              padding: '4px 14px',
              fontSize: 11,
              color: 'var(--fg-dim)',
              fontStyle: 'italic',
            }}
          >
            No active sessions.
          </div>
        ) : (
          sessions.map((s) => <SessionRow key={`${s.kind}:${s.id}`} s={s} />)
        )}
      </div>
    </div>
  )
}

// ── Right column: toolbar + event log ──────────────────────────────────

function CheckIcon() {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  )
}

function Toolbar({ filteredCount }: { filteredCount: number }) {
  const filter = useProcessesStore((s) => s.filter)
  const follow = useProcessesStore((s) => s.follow)
  const eventsLen = useProcessesStore((s) => s.events.length)
  const setFilter = useProcessesStore((s) => s.setFilter)
  const setFollow = useProcessesStore((s) => s.setFollow)
  const clearEvents = useProcessesStore((s) => s.clearEvents)

  return (
    <div
      style={{
        height: 36,
        flexShrink: 0,
        borderBottom: '1px solid var(--line-soft)',
        display: 'flex',
        alignItems: 'center',
        padding: '0 12px',
        gap: 12,
        background: 'var(--bg)',
      }}
    >
      <input
        type="search"
        placeholder="Filter events…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        style={{
          flex: 1,
          minWidth: 0,
          maxWidth: 320,
          height: 24,
          padding: '0 8px',
          background: 'var(--bg-raised)',
          color: 'var(--fg)',
          border: '1px solid var(--line-soft)',
          borderRadius: 4,
          fontSize: 12,
          fontFamily: 'var(--f-ui)',
          outline: 'none',
        }}
      />

      <button
        onClick={() => setFollow(!follow)}
        title={follow ? 'Auto-scroll enabled' : 'Auto-scroll disabled'}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 4,
          height: 24,
          padding: '0 8px',
          background: follow ? 'var(--accent-soft)' : 'transparent',
          color: follow ? 'var(--accent)' : 'var(--fg-muted)',
          border: '1px solid var(--line-soft)',
          borderRadius: 4,
          fontSize: 11,
          fontFamily: 'var(--f-ui)',
          cursor: 'pointer',
        }}
      >
        {follow && <CheckIcon />}
        Follow
      </button>

      <button
        onClick={clearEvents}
        title="Clear event buffer"
        style={{
          height: 24,
          padding: '0 8px',
          background: 'transparent',
          color: 'var(--fg-muted)',
          border: '1px solid var(--line-soft)',
          borderRadius: 4,
          fontSize: 11,
          fontFamily: 'var(--f-ui)',
          cursor: 'pointer',
        }}
      >
        Clear
      </button>

      <div
        style={{
          marginLeft: 'auto',
          color: 'var(--fg-dim)',
          fontFamily: 'var(--f-mono)',
          fontSize: 11,
        }}
      >
        {filter.length > 0 ? `${filteredCount} / ` : ''}
        {eventsLen} / {PROCESS_EVENTS_CAP}
      </div>
    </div>
  )
}

function EventLog() {
  const events = useProcessesStore((s) => s.events)
  const filter = useProcessesStore((s) => s.filter)
  const follow = useProcessesStore((s) => s.follow)
  const containerRef = useRef<HTMLDivElement | null>(null)

  const filtered = useMemo(() => {
    if (filter.trim().length === 0) return events
    const needle = filter.toLowerCase()
    return events.filter(
      (e) =>
        e.topic.toLowerCase().includes(needle) ||
        e.payloadJson.toLowerCase().includes(needle),
    )
  }, [events, filter])

  // Auto-scroll to the bottom on new events when follow is on. We only
  // scroll when `follow` is true — the user can still manually scroll
  // up by toggling Follow off first. Running on `filtered.length`
  // rather than `events.length` keeps the bottom-anchor stable when a
  // filter is active and new matching events land.
  useEffect(() => {
    if (!follow) return
    const el = containerRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [filtered.length, follow])

  if (events.length === 0) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--fg-dim)',
          fontSize: 12,
          fontFamily: 'var(--f-ui)',
        }}
      >
        No events captured yet.
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
          color: 'var(--fg-dim)',
          fontSize: 12,
          fontFamily: 'var(--f-ui)',
        }}
      >
        No events match.
      </div>
    )
  }

  return (
    <div
      ref={containerRef}
      style={{
        flex: 1,
        overflowY: 'auto',
        padding: '4px 0',
        background: 'var(--bg)',
      }}
    >
      {filtered.map((e, i) => (
        <div
          key={i}
          style={{
            display: 'flex',
            alignItems: 'baseline',
            gap: 10,
            padding: '2px 12px',
            fontSize: 11,
            lineHeight: 1.5,
            whiteSpace: 'nowrap',
          }}
        >
          <span style={{ color: 'var(--fg-dim)', fontFamily: 'var(--f-mono)', flexShrink: 0 }}>
            {formatTs(e.timestampMs)}
          </span>
          <span style={{ color: 'var(--fg)', fontFamily: 'var(--f-ui)', flexShrink: 0 }}>
            {e.topic}
          </span>
          <span
            style={{
              color: 'var(--fg-muted)',
              fontFamily: 'var(--f-mono)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              flex: 1,
              minWidth: 0,
            }}
          >
            {truncate(e.payloadJson, 120)}
          </span>
        </div>
      ))}
    </div>
  )
}

function RightColumn() {
  const events = useProcessesStore((s) => s.events)
  const filter = useProcessesStore((s) => s.filter)

  const filteredCount = useMemo(() => {
    if (filter.trim().length === 0) return events.length
    const needle = filter.toLowerCase()
    return events.reduce(
      (n, e) =>
        e.topic.toLowerCase().includes(needle) ||
        e.payloadJson.toLowerCase().includes(needle)
          ? n + 1
          : n,
      0,
    )
  }, [events, filter])

  return (
    <div
      style={{
        flex: 1,
        minWidth: 0,
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--bg)',
      }}
    >
      <Toolbar filteredCount={filteredCount} />
      <EventLog />
    </div>
  )
}

// ── Root ────────────────────────────────────────────────────────────────

export function ProcessesView() {
  return (
    <div
      style={{
        display: 'flex',
        height: '100%',
        width: '100%',
        background: 'var(--bg)',
        color: 'var(--fg)',
        fontFamily: 'var(--f-ui)',
      }}
    >
      <LeftColumn />
      <RightColumn />
    </div>
  )
}
