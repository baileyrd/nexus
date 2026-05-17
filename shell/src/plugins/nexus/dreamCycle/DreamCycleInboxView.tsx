// BL-129 follow-up — Dream Cycle inbox view.
//
// Renders one row per LLM-proposed relation (confidence ≤ 0.5) with
// Approve / Skip buttons. The plugin's activate hook owns the
// hydration + IPC lifecycle; this component is render-only.

import { useDreamCycleStore, rowKey, type DraftRelationRow } from './dreamCycleStore'

interface Props {
  onApprove(row: DraftRelationRow): void
  onSkip(row: DraftRelationRow): void
  onRefresh(): void
}

function formatConfidence(c: number): string {
  if (!Number.isFinite(c)) return '—'
  return c.toFixed(2)
}

export function DreamCycleInboxView(props: Props) {
  const rows = useDreamCycleStore((s) => s.rows)
  const total = useDreamCycleStore((s) => s.total)
  const truncated = useDreamCycleStore((s) => s.truncated)
  const hydrated = useDreamCycleStore((s) => s.hydrated)
  const pending = useDreamCycleStore((s) => s.pending)

  return (
    <div
      className="nexus-dream-cycle-inbox"
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
          borderBottom: '1px solid var(--border, #2a2a2a)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
        }}
      >
        <div>
          <strong style={{ fontSize: 14 }}>Dream Cycle</strong>
          <span
            style={{ marginLeft: 8, color: 'var(--text-muted, #888)' }}
          >
            {rows.length} {rows.length === 1 ? 'proposal' : 'proposals'}
            {truncated ? ` of ${total}` : ''}
          </span>
        </div>
        <button
          type="button"
          onClick={props.onRefresh}
          style={{
            background: 'transparent',
            color: 'var(--text-muted, #888)',
            border: '1px solid var(--border, #2a2a2a)',
            borderRadius: 4,
            padding: '2px 8px',
            cursor: 'pointer',
            fontSize: 12,
          }}
        >
          Refresh
        </button>
      </header>

      {!hydrated && (
        <div
          style={{
            padding: 16,
            color: 'var(--text-muted, #888)',
          }}
        >
          Loading…
        </div>
      )}

      {hydrated && rows.length === 0 && (
        <div
          style={{
            padding: 16,
            color: 'var(--text-muted, #888)',
          }}
        >
          No pending Dream Cycle proposals. New relations from the
          nightly cycle will appear here for approval.
        </div>
      )}

      {hydrated && rows.length > 0 && (
        <ul
          style={{
            listStyle: 'none',
            margin: 0,
            padding: 0,
            overflowY: 'auto',
            flex: 1,
          }}
        >
          {rows.map((row) => {
            const key = rowKey(row)
            const isPending = pending.has(key)
            return (
              <li
                key={key}
                style={{
                  padding: '8px 12px',
                  borderBottom: '1px solid var(--border, #2a2a2a)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 6,
                  opacity: isPending ? 0.5 : 1,
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'baseline',
                    flexWrap: 'wrap',
                    gap: 6,
                  }}
                >
                  <strong>{row.from}</strong>
                  <code
                    style={{
                      background: 'var(--surface-soft, #1f1f1f)',
                      borderRadius: 3,
                      padding: '1px 6px',
                      fontSize: 12,
                    }}
                  >
                    {row.type}
                  </code>
                  <strong>{row.target}</strong>
                  <span
                    style={{
                      marginLeft: 'auto',
                      color: 'var(--text-muted, #888)',
                      fontSize: 12,
                    }}
                  >
                    confidence {formatConfidence(row.confidence)}
                  </span>
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  <button
                    type="button"
                    disabled={isPending}
                    onClick={() => props.onApprove(row)}
                    style={{
                      background: 'var(--interactive-accent, #3b82f6)',
                      color: 'var(--text-on-accent, white)',
                      border: 'none',
                      borderRadius: 4,
                      padding: '2px 12px',
                      cursor: isPending ? 'wait' : 'pointer',
                      fontSize: 12,
                    }}
                  >
                    Approve
                  </button>
                  <button
                    type="button"
                    disabled={isPending}
                    onClick={() => props.onSkip(row)}
                    style={{
                      background: 'transparent',
                      color: 'var(--text-muted, #888)',
                      border: '1px solid var(--border, #2a2a2a)',
                      borderRadius: 4,
                      padding: '2px 12px',
                      cursor: isPending ? 'wait' : 'pointer',
                      fontSize: 12,
                    }}
                  >
                    Skip
                  </button>
                </div>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
