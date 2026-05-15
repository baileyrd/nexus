import { useMemo } from 'react'
import {
  deriveStats,
  parsePayloadTaskId,
  useNotificationsInboxStore,
  type InboxEntry,
  type InboxSeverity,
} from './notificationsInboxStore'

/**
 * BL-136 Phase 2 — Notification Center view.
 *
 * Three regions:
 *
 *   ┌─────────────────────────────────────────────────────────────┐
 *   │ NOTIFICATIONS [unread: N]   [Mark all read]                 │
 *   │ [All] [workflow N] [ai_runtime N] [override N] ...          │
 *   ├─────────────────────────────────────────────────────────────┤
 *   │ • 14:23  workflow • warn  "Backup failed: 502"     [✓] [×] │
 *   │   channels: desktop, telegram                               │
 *   │ ◦ 14:01  ai_runtime • info "Task abc-123 finished"  [✓] [×]│
 *   │   [Jump to run →]                                           │
 *   └─────────────────────────────────────────────────────────────┘
 *
 * The view is render-only; the plugin's activate hook owns the
 * hydrate-on-open + bus-subscribe lifecycle.
 */

interface Props {
  onMarkRead(ids: string[]): void
  onDismiss(ids: string[]): void
  onJumpToTask(taskId: string): void
}

function formatTimestamp(ts: number): string {
  const d = new Date(ts * 1000)
  if (Number.isNaN(d.getTime())) return String(ts)
  const hh = String(d.getHours()).padStart(2, '0')
  const mm = String(d.getMinutes()).padStart(2, '0')
  return `${hh}:${mm}`
}

function severityColor(s: InboxSeverity): string {
  switch (s) {
    case 'error':
      return 'var(--error, #dc2626)'
    case 'warn':
      return 'var(--warm, #d97706)'
    case 'info':
      return 'var(--interactive-accent, #3b82f6)'
    case 'debug':
    default:
      return 'var(--text-muted, #888)'
  }
}

export function NotificationsInboxView(props: Props) {
  const entries = useNotificationsInboxStore((s) => s.entries)
  const hydrated = useNotificationsInboxStore((s) => s.hydrated)
  const sourceFilter = useNotificationsInboxStore((s) => s.sourceFilter)
  const setSourceFilter = useNotificationsInboxStore((s) => s.setSourceFilter)

  const stats = useMemo(() => deriveStats(entries), [entries])

  const visible = useMemo(() => {
    const list = entries.filter((e) => e.dismissed_at === null)
    if (sourceFilter === null) return list
    return list.filter((e) => e.source === sourceFilter)
  }, [entries, sourceFilter])

  const sources = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const e of entries) {
      if (e.dismissed_at !== null) continue
      counts[e.source] = (counts[e.source] ?? 0) + 1
    }
    return Object.entries(counts).sort(([a], [b]) => a.localeCompare(b))
  }, [entries])

  const unreadIds = useMemo(
    () =>
      entries
        .filter((e) => e.dismissed_at === null && e.read_at === null)
        .map((e) => e.id),
    [entries],
  )

  return (
    <div
      className="nexus-notifications-inbox"
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        fontSize: 13,
      }}
    >
      <header
        style={{
          padding: '8px 12px',
          borderBottom: '1px solid var(--background-modifier-border, #2a2a2a)',
          display: 'flex',
          alignItems: 'center',
          gap: 12,
        }}
      >
        <strong style={{ fontSize: 14 }}>Notifications</strong>
        <span style={{ color: 'var(--text-muted)' }}>
          {stats.unread} unread / {stats.total} total
        </span>
        <span style={{ flex: 1 }} />
        <button
          type="button"
          disabled={unreadIds.length === 0}
          onClick={() => props.onMarkRead(unreadIds)}
          aria-label="Mark all as read"
        >
          Mark all read
        </button>
      </header>

      <nav
        style={{
          padding: '6px 12px',
          display: 'flex',
          gap: 6,
          flexWrap: 'wrap',
          borderBottom: '1px solid var(--background-modifier-border, #2a2a2a)',
        }}
        aria-label="Filter by source"
      >
        <Chip
          active={sourceFilter === null}
          onClick={() => setSourceFilter(null)}
          label={`All ${stats.total}`}
        />
        {sources.map(([src, count]) => (
          <Chip
            key={src}
            active={sourceFilter === src}
            onClick={() => setSourceFilter(src)}
            label={`${src} ${count}`}
          />
        ))}
      </nav>

      <ul
        style={{
          flex: 1,
          overflowY: 'auto',
          margin: 0,
          padding: 0,
          listStyle: 'none',
        }}
      >
        {!hydrated && (
          <li style={emptyStyle}>
            <span style={{ color: 'var(--text-muted)' }}>Loading…</span>
          </li>
        )}
        {hydrated && visible.length === 0 && (
          <li style={emptyStyle}>
            <span style={{ color: 'var(--text-muted)' }}>
              {sourceFilter === null
                ? 'No notifications.'
                : `No notifications from "${sourceFilter}".`}
            </span>
          </li>
        )}
        {visible.map((e) => (
          <Row
            key={e.id}
            entry={e}
            onMarkRead={() => props.onMarkRead([e.id])}
            onDismiss={() => props.onDismiss([e.id])}
            onJumpToTask={props.onJumpToTask}
          />
        ))}
      </ul>
    </div>
  )
}

const emptyStyle: React.CSSProperties = {
  padding: '24px 12px',
  textAlign: 'center',
}

function Chip(props: {
  active: boolean
  onClick(): void
  label: string
}) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      style={{
        padding: '2px 8px',
        borderRadius: 12,
        border: '1px solid var(--background-modifier-border, #2a2a2a)',
        background: props.active
          ? 'var(--interactive-accent, #3b82f6)'
          : 'transparent',
        color: props.active ? 'white' : 'var(--text-normal)',
        fontSize: 12,
        cursor: 'pointer',
      }}
    >
      {props.label}
    </button>
  )
}

function Row(props: {
  entry: InboxEntry
  onMarkRead(): void
  onDismiss(): void
  onJumpToTask(taskId: string): void
}) {
  const { entry } = props
  const unread = entry.read_at === null
  const taskId = parsePayloadTaskId(entry.payload_json)

  return (
    <li
      data-id={entry.id}
      data-unread={unread ? 'true' : 'false'}
      onClick={() => {
        if (unread) props.onMarkRead()
      }}
      style={{
        padding: '8px 12px',
        borderBottom: '1px solid var(--background-modifier-border, #2a2a2a)',
        cursor: unread ? 'pointer' : 'default',
        opacity: unread ? 1 : 0.65,
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
        <span
          aria-hidden
          style={{
            color: severityColor(entry.severity),
            fontWeight: unread ? 600 : 400,
          }}
        >
          {unread ? '●' : '○'}
        </span>
        <span style={{ color: 'var(--text-muted)', fontSize: 11 }}>
          {formatTimestamp(entry.ts)}
        </span>
        <span style={{ fontWeight: 500 }}>{entry.source}</span>
        <span
          style={{
            color: severityColor(entry.severity),
            fontSize: 11,
            textTransform: 'uppercase',
          }}
        >
          {entry.severity}
        </span>
        <span style={{ flex: 1 }} />
        {unread && (
          <button
            type="button"
            onClick={(ev) => {
              ev.stopPropagation()
              props.onMarkRead()
            }}
            aria-label="Mark as read"
            style={iconButtonStyle}
          >
            ✓
          </button>
        )}
        <button
          type="button"
          onClick={(ev) => {
            ev.stopPropagation()
            props.onDismiss()
          }}
          aria-label="Dismiss"
          style={iconButtonStyle}
        >
          ×
        </button>
      </div>
      <div style={{ paddingLeft: 22 }}>
        {entry.title && (
          <div style={{ fontWeight: 500 }}>{entry.title}</div>
        )}
        <div>{entry.body}</div>
        {entry.channels.length > 0 && (
          <div style={{ color: 'var(--text-muted)', fontSize: 11, marginTop: 2 }}>
            routed to: {entry.channels.join(', ')}
          </div>
        )}
        {taskId && (
          <button
            type="button"
            onClick={(ev) => {
              ev.stopPropagation()
              props.onJumpToTask(taskId)
            }}
            style={{
              marginTop: 4,
              fontSize: 11,
              background: 'transparent',
              border: 'none',
              color: 'var(--interactive-accent)',
              cursor: 'pointer',
              padding: 0,
            }}
          >
            Jump to run →
          </button>
        )}
      </div>
    </li>
  )
}

const iconButtonStyle: React.CSSProperties = {
  background: 'transparent',
  border: '1px solid var(--background-modifier-border, #2a2a2a)',
  borderRadius: 4,
  width: 22,
  height: 22,
  cursor: 'pointer',
  fontSize: 14,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 0,
}
