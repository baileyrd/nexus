// C84 (#437) — per-plugin audit/denial timeline. Every capability-gated
// event already persists to .forge/.kernel/audit.db and is queryable via
// com.nexus.security::query_audit_log (the CLI's `nexus logs` has used
// it for a while); this is the first shell surface to call it. The
// shell's kernel_invoke bridge runs every call through the host's own
// all-capabilities context (crates/nexus-bootstrap/src/lib.rs), so no
// new capability grant is needed here.

import { useEffect, useMemo, useState } from 'react'
import { useAuditLogStore } from './auditLogStore'
import { getApi } from './pluginsMgmtRuntime'

const SECURITY_PLUGIN_ID = 'com.nexus.security'
const FETCH_LIMIT = 500

interface AuditLogEntry {
  id: number | string
  ts_ms: number | string
  event_type: string
  plugin_id: string | null
  detail_json: string
}

export function formatTs(tsMs: number | string): string {
  const ms = typeof tsMs === 'string' ? Number(tsMs) : tsMs
  if (!Number.isFinite(ms)) return String(tsMs)
  return new Date(ms).toLocaleString()
}

/** Pretty-print `detail_json` when it parses; fall back to the raw string. */
export function formatDetail(detailJson: string): string {
  try {
    return JSON.stringify(JSON.parse(detailJson))
  } catch {
    return detailJson
  }
}

export function isDenialEvent(eventType: string): boolean {
  return eventType.includes('denied')
}

/** Client-side substring filter over event_type/detail_json — the
 *  "filterable" half of the per-plugin audit timeline (C84 / #437). */
export function filterAuditEntries<
  T extends { event_type: string; detail_json: string },
>(entries: T[], query: string): T[] {
  const q = query.trim().toLowerCase()
  if (!q) return entries
  return entries.filter(
    (e) =>
      e.event_type.toLowerCase().includes(q) ||
      e.detail_json.toLowerCase().includes(q),
  )
}

export function AuditLogModal() {
  const pluginId = useAuditLogStore((s) => s.pluginId)
  const close = useAuditLogStore((s) => s.close)

  const [entries, setEntries] = useState<AuditLogEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [filter, setFilter] = useState('')
  const [reloadNonce, setReloadNonce] = useState(0)

  useEffect(() => {
    if (!pluginId) return
    let cancelled = false
    setLoading(true)
    setError(null)
    getApi()
      .kernel.invoke<AuditLogEntry[]>(SECURITY_PLUGIN_ID, 'query_audit_log', {
        plugin_id: pluginId,
        limit: FETCH_LIMIT,
      })
      .then((rows) => {
        if (cancelled) return
        setEntries(Array.isArray(rows) ? rows : [])
      })
      .catch((e: unknown) => {
        if (cancelled) return
        setError(String((e as Error)?.message ?? e))
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [pluginId, reloadNonce])

  // Reset transient view state each time a different plugin's log opens.
  useEffect(() => {
    setFilter('')
    setEntries([])
    setError(null)
  }, [pluginId])

  const filtered = useMemo(
    () => filterAuditEntries(entries, filter),
    [entries, filter],
  )

  if (!pluginId) return null

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) close()
  }

  return (
    <div
      onClick={onBackdropClick}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'oklch(0 0 0 / 0.45)',
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'flex-start',
        paddingTop: 100,
        zIndex: 1000,
      }}
    >
      <div
        style={{
          width: 640,
          maxWidth: '90vw',
          maxHeight: '70vh',
          background: 'var(--background-secondary)',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 'var(--radius-l)',
          boxShadow: 'var(--shadow)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 12,
            padding: '12px 16px',
            borderBottom: '1px solid var(--divider-color)',
          }}
        >
          <div style={{ minWidth: 0 }}>
            <div
              style={{
                color: 'var(--text-normal)',
                fontFamily: 'var(--font-interface)',
                fontSize: 14,
                fontWeight: 600,
              }}
            >
              Audit log
            </div>
            <div
              style={{
                color: 'var(--text-faint)',
                fontFamily: 'var(--font-monospace)',
                fontSize: 11,
                marginTop: 2,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {pluginId}
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
            <button
              type="button"
              onClick={() => setReloadNonce((n) => n + 1)}
              title="Reload"
              style={{
                padding: '2px 8px',
                background: 'transparent',
                color: 'var(--text-faint)',
                border: '1px solid var(--divider-color)',
                borderRadius: 'var(--radius-s)',
                fontFamily: 'var(--font-interface)',
                fontSize: 11,
                cursor: 'pointer',
              }}
            >
              Reload
            </button>
            <button
              type="button"
              onClick={close}
              title="Close"
              style={{
                padding: '2px 8px',
                background: 'transparent',
                color: 'var(--text-faint)',
                border: '1px solid var(--divider-color)',
                borderRadius: 'var(--radius-s)',
                fontFamily: 'var(--font-interface)',
                fontSize: 11,
                cursor: 'pointer',
              }}
            >
              Close
            </button>
          </div>
        </div>

        <div style={{ padding: '8px 16px', borderBottom: '1px solid var(--divider-color)' }}>
          <input
            type="search"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter by event type or detail…"
            spellCheck={false}
            autoComplete="off"
            style={{
              width: '100%',
              background: 'var(--background-primary)',
              border: '1px solid var(--divider-color)',
              borderRadius: 'var(--radius-s)',
              padding: '4px 8px',
              color: 'var(--text-normal)',
              fontFamily: 'var(--font-interface)',
              fontSize: 12,
            }}
          />
        </div>

        <div style={{ flex: 1, minHeight: 0, overflowY: 'auto', padding: '4px 0' }}>
          {loading ? (
            <div style={emptyStateStyle}>Loading…</div>
          ) : error ? (
            <div style={{ ...emptyStateStyle, color: 'var(--risk)' }}>{error}</div>
          ) : filtered.length === 0 ? (
            <div style={emptyStateStyle}>
              {entries.length === 0
                ? 'No audit entries for this plugin yet.'
                : 'No entries match the filter.'}
            </div>
          ) : (
            filtered.map((entry) => (
              <div
                key={String(entry.id)}
                style={{
                  padding: '6px 16px',
                  borderBottom: '1px solid var(--background-modifier-border-hover, transparent)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 2,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span
                    style={{
                      fontFamily: 'var(--font-monospace)',
                      fontSize: 11,
                      color: isDenialEvent(entry.event_type)
                        ? 'var(--risk)'
                        : 'var(--text-normal)',
                      fontWeight: isDenialEvent(entry.event_type) ? 600 : 400,
                    }}
                  >
                    {entry.event_type}
                  </span>
                  <span
                    style={{
                      fontFamily: 'var(--font-interface)',
                      fontSize: 11,
                      color: 'var(--text-faint)',
                      marginLeft: 'auto',
                    }}
                  >
                    {formatTs(entry.ts_ms)}
                  </span>
                </div>
                <div
                  title={entry.detail_json}
                  style={{
                    fontFamily: 'var(--font-monospace)',
                    fontSize: 11,
                    color: 'var(--text-muted)',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {formatDetail(entry.detail_json)}
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  )
}

const emptyStateStyle: React.CSSProperties = {
  padding: '24px 16px',
  textAlign: 'center',
  color: 'var(--text-faint)',
  fontFamily: 'var(--font-interface)',
  fontSize: 12,
}
