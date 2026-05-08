// BL-054 Phase 4 — observability panel view.
//
// Three internal tabs (Usage / Automation / Vault feed) each backed by
// the observabilityStore. Refresh is driven by the parent plugin's
// `index.ts`; the view is purely presentational.

import { useObservabilityStore, type ObservabilityTab, type VaultFeedEntry } from './observabilityStore'
import type { DailyCounts, SurfaceCounts } from './usageAggregate'

interface Props {
  onRefreshUsage: () => void
  onRefreshAutomation: () => void
  onRunWorkflow: (name: string) => void
}

export function OsObservabilityView({
  onRefreshUsage,
  onRefreshAutomation,
  onRunWorkflow,
}: Props) {
  const activeTab = useObservabilityStore((s) => s.activeTab)
  const setActiveTab = useObservabilityStore((s) => s.setActiveTab)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        fontFamily: 'var(--font-interface)',
        color: 'var(--text-normal)',
        background: 'var(--background-primary)',
      }}
    >
      <Tabs activeTab={activeTab} onChange={setActiveTab} />
      <div style={{ flex: 1, overflowY: 'auto' }}>
        {activeTab === 'usage' && <UsageTab onRefresh={onRefreshUsage} />}
        {activeTab === 'automation' && (
          <AutomationTab onRefresh={onRefreshAutomation} onRun={onRunWorkflow} />
        )}
        {activeTab === 'vault' && <VaultTab />}
      </div>
    </div>
  )
}

function Tabs({
  activeTab,
  onChange,
}: {
  activeTab: ObservabilityTab
  onChange: (t: ObservabilityTab) => void
}) {
  const items: { id: ObservabilityTab; label: string }[] = [
    { id: 'usage', label: 'Usage' },
    { id: 'automation', label: 'Automation' },
    { id: 'vault', label: 'Vault feed' },
  ]
  return (
    <div
      style={{
        display: 'flex',
        gap: 0,
        borderBottom: '1px solid var(--background-modifier-border)',
        flexShrink: 0,
        padding: '0 6px',
      }}
    >
      {items.map((it) => (
        <button
          key={it.id}
          type="button"
          onClick={() => onChange(it.id)}
          style={{
            background: 'transparent',
            border: 0,
            borderBottom:
              activeTab === it.id
                ? '2px solid var(--interactive-accent)'
                : '2px solid transparent',
            color:
              activeTab === it.id ? 'var(--text-normal)' : 'var(--text-muted)',
            fontFamily: 'var(--font-interface)',
            fontSize: 12,
            fontWeight: activeTab === it.id ? 600 : 400,
            padding: '8px 12px 6px',
            cursor: 'pointer',
            flexShrink: 0,
          }}
        >
          {it.label}
        </button>
      ))}
    </div>
  )
}

// ─── Usage tab ─────────────────────────────────────────────────────────

function UsageTab({ onRefresh }: { onRefresh: () => void }) {
  const loading = useObservabilityStore((s) => s.usageLoading)
  const error = useObservabilityStore((s) => s.usageError)
  const rollup = useObservabilityStore((s) => s.usageRollup)
  const total = rollup?.total ?? 0

  return (
    <div style={{ padding: 12 }}>
      <PaneHeader
        title={`USAGE — ${total} entries`}
        subtitle={
          rollup?.latest
            ? `Latest: ${formatTimestamp(rollup.latest)}`
            : 'No activity recorded yet.'
        }
        onRefresh={onRefresh}
      />
      {error && <ErrorBanner message={error} />}
      {loading && total === 0 && <Loading />}
      {rollup && rollup.bySurface.length > 0 && (
        <>
          <SectionHeader>Per surface</SectionHeader>
          <SurfaceTable entries={rollup.bySurface} />
          <SectionHeader>Last 14 days</SectionHeader>
          <DayBars entries={rollup.byDay} />
        </>
      )}
      {rollup && rollup.bySurface.length === 0 && !loading && (
        <Empty>
          The activity log is empty. Talk to the AI, run a workflow, or save a
          file in the forge — entries will surface here.
        </Empty>
      )}
      <Footnote>
        Token / cost columns are intentionally absent: the activity schema
        doesn't carry that data today (BL-054 Phase 4 deferred follow-up).
      </Footnote>
    </div>
  )
}

function SurfaceTable({ entries }: { entries: SurfaceCounts[] }) {
  const max = Math.max(1, ...entries.map((e) => e.total))
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      {entries.map((row) => (
        <div
          key={row.surface}
          style={{
            display: 'grid',
            gridTemplateColumns: '90px 1fr auto auto',
            alignItems: 'center',
            gap: 8,
            fontSize: 12,
          }}
        >
          <span
            style={{
              fontFamily: 'var(--font-monospace)',
              fontSize: 11,
              color: 'var(--text-muted)',
            }}
          >
            {row.surface}
          </span>
          <div
            style={{
              height: 6,
              background: 'var(--background-secondary)',
              borderRadius: 3,
              overflow: 'hidden',
            }}
          >
            <div
              style={{
                width: `${(row.total / max) * 100}%`,
                height: '100%',
                background: 'var(--interactive-accent)',
              }}
            />
          </div>
          <span style={{ fontFamily: 'var(--font-monospace)', fontSize: 11 }}>
            {row.total}
          </span>
          <span
            style={{
              fontFamily: 'var(--font-monospace)',
              fontSize: 11,
              color: row.error > 0 ? 'var(--risk)' : 'var(--text-faint)',
              minWidth: 40,
              textAlign: 'right',
            }}
            title={`${row.ok} ok · ${row.error} error · ${row.cancelled} cancelled`}
          >
            {row.error > 0 ? `${row.error} err` : 'all ok'}
          </span>
        </div>
      ))}
    </div>
  )
}

function DayBars({ entries }: { entries: DailyCounts[] }) {
  const max = Math.max(1, ...entries.map((e) => e.total))
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-end',
        gap: 3,
        height: 60,
        marginTop: 4,
      }}
    >
      {entries.map((d) => {
        const okHeight = (d.ok / max) * 100
        const errHeight = (d.error / max) * 100
        return (
          <div
            key={d.date}
            title={`${d.date} — ${d.total} total (${d.ok} ok, ${d.error} err)`}
            style={{
              flex: 1,
              minWidth: 0,
              display: 'flex',
              flexDirection: 'column',
              justifyContent: 'flex-end',
              gap: 1,
              height: '100%',
            }}
          >
            <div
              style={{
                height: `${errHeight}%`,
                background: 'var(--risk)',
                borderRadius: 2,
              }}
            />
            <div
              style={{
                height: `${okHeight}%`,
                background: 'var(--interactive-accent)',
                borderRadius: 2,
              }}
            />
          </div>
        )
      })}
    </div>
  )
}

// ─── Automation tab ───────────────────────────────────────────────────

function AutomationTab({
  onRefresh,
  onRun,
}: {
  onRefresh: () => void
  onRun: (name: string) => void
}) {
  const loading = useObservabilityStore((s) => s.automationLoading)
  const error = useObservabilityStore((s) => s.automationError)
  const all = useObservabilityStore((s) => s.automationWorkflows)
  const lastRun = useObservabilityStore((s) => s.automationLastRun)
  const nextFire = useObservabilityStore((s) => s.automationNextFire)
  // Foundation workflows — those whose trigger fires automatically.
  const foundations = all.filter(
    (w) => w.triggerType === 'cron' || w.triggerType === 'file_event',
  )

  return (
    <div style={{ padding: 12 }}>
      <PaneHeader
        title={`AUTOMATION — ${foundations.length} foundation${foundations.length === 1 ? '' : 's'}`}
        subtitle="Workflows whose trigger is `cron` or `file_event`."
        onRefresh={onRefresh}
      />
      {error && <ErrorBanner message={error} />}
      {loading && foundations.length === 0 && <Loading />}
      {foundations.length === 0 && !loading && (
        <Empty>
          No foundation workflows. Add a `.workflow.toml` under
          `.forge/workflows/` with a `cron` or `file_event` trigger.
        </Empty>
      )}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        {foundations.map((wf) => {
          const run = lastRun[wf.name] ?? null
          return (
            <div
              key={wf.name}
              style={{
                padding: '8px 10px',
                border: '1px solid var(--divider-color)',
                borderRadius: 6,
                background: 'var(--background-secondary)',
                display: 'flex',
                flexDirection: 'column',
                gap: 4,
              }}
            >
              <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
                <span
                  style={{
                    fontFamily: 'var(--font-monospace)',
                    fontSize: 12,
                    color: 'var(--text-normal)',
                    flex: 1,
                  }}
                >
                  {wf.name}
                </span>
                <span
                  style={{
                    fontFamily: 'var(--font-monospace)',
                    fontSize: 10,
                    padding: '1px 6px',
                    borderRadius: 999,
                    background: 'var(--ok-soft)',
                    color: 'var(--ok)',
                    border: '1px solid var(--ok-soft)',
                  }}
                >
                  {wf.triggerType}
                </span>
                <button
                  type="button"
                  onClick={() => onRun(wf.name)}
                  style={{
                    fontFamily: 'var(--font-interface)',
                    fontSize: 11,
                    padding: '2px 8px',
                    border: '1px solid var(--divider-color)',
                    borderRadius: 4,
                    background: 'transparent',
                    color: 'var(--text-normal)',
                    cursor: 'pointer',
                  }}
                  title="Manually invoke this workflow"
                >
                  Run now
                </button>
              </div>
              {wf.description && (
                <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                  {wf.description}
                </div>
              )}
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  fontSize: 11,
                  color: 'var(--text-faint)',
                }}
              >
                <span>
                  {wf.stepCount} step{wf.stepCount === 1 ? '' : 's'}
                </span>
                <span>·</span>
                <LastRun record={run} />
                {wf.triggerType === 'cron' && (
                  <>
                    <span>·</span>
                    <NextFire iso={nextFire[wf.name] ?? null} />
                  </>
                )}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

/**
 * BL-054 Phase 4 follow-up — render the next computed fire time for
 * cron-triggered workflows. The kernel returns RFC-3339 UTC; we
 * surface a humanised relative form ("in 3 hours") plus an absolute
 * `<time>` tooltip so the user can read either form.
 */
function NextFire({ iso }: { iso: string | null }) {
  if (!iso) {
    return (
      <span style={{ color: 'var(--text-faint)' }} title="schedule unparseable">
        next: —
      </span>
    )
  }
  const dt = new Date(iso)
  if (Number.isNaN(dt.getTime())) {
    return <span style={{ color: 'var(--text-faint)' }}>next: —</span>
  }
  const now = Date.now()
  const deltaMs = dt.getTime() - now
  return (
    <time
      dateTime={iso}
      title={dt.toLocaleString()}
      style={{ color: 'var(--text-muted)' }}
    >
      next: {formatRelative(deltaMs)}
    </time>
  )
}

function formatRelative(ms: number): string {
  if (!Number.isFinite(ms)) return '—'
  const absMs = Math.abs(ms)
  const future = ms >= 0
  const minutes = Math.round(absMs / 60_000)
  if (minutes < 1) return future ? 'in <1m' : 'just now'
  if (minutes < 60) return future ? `in ${minutes}m` : `${minutes}m ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return future ? `in ${hours}h` : `${hours}h ago`
  const days = Math.round(hours / 24)
  if (days < 30) return future ? `in ${days}d` : `${days}d ago`
  const months = Math.round(days / 30)
  return future ? `in ${months}mo` : `${months}mo ago`
}

function LastRun({
  record,
}: {
  record: { finishedAt: string; success: boolean; conditionSkipped: boolean; error: string | null } | null
}) {
  if (!record) {
    return <span style={{ color: 'var(--text-faint)' }}>never run</span>
  }
  const colour = record.conditionSkipped
    ? 'var(--text-muted)'
    : record.success
      ? 'var(--ok)'
      : 'var(--risk)'
  const label = record.conditionSkipped
    ? 'skipped'
    : record.success
      ? 'ok'
      : 'failed'
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: '50%',
          background: colour,
          flexShrink: 0,
        }}
      />
      <span title={record.error ?? undefined}>
        {label} · {formatTimestamp(record.finishedAt)}
      </span>
    </span>
  )
}

// ─── Vault feed tab ───────────────────────────────────────────────────

function VaultTab() {
  const entries = useObservabilityStore((s) => s.vaultEntries)
  return (
    <div style={{ padding: 12 }}>
      <PaneHeader
        title={`VAULT FEED — ${entries.length} event${entries.length === 1 ? '' : 's'}`}
        subtitle="File-create / modify / delete activity under raw/, wiki/, and output/."
      />
      {entries.length === 0 ? (
        <Empty>No file activity yet. Save a file under raw/, wiki/, or output/.</Empty>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {entries.map((e) => (
            <FeedRow key={e.id} entry={e} />
          ))}
        </div>
      )}
    </div>
  )
}

function FeedRow({ entry }: { entry: VaultFeedEntry }) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '110px 1fr',
        gap: 8,
        padding: '4px 0',
        borderBottom: '1px dashed var(--divider-color)',
        fontSize: 12,
      }}
    >
      <span
        style={{
          fontFamily: 'var(--font-monospace)',
          fontSize: 11,
          color: 'var(--text-faint)',
        }}
        title={entry.timestamp}
      >
        {formatTimestamp(entry.timestamp)}
      </span>
      <span style={{ color: 'var(--text-normal)' }}>{entry.prompt}</span>
    </div>
  )
}

// ─── Shared bits ──────────────────────────────────────────────────────

function PaneHeader({
  title,
  subtitle,
  onRefresh,
}: {
  title: string
  subtitle: string
  onRefresh?: () => void
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12, marginBottom: 12 }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: '0.06em',
            color: 'var(--text-muted)',
          }}
        >
          {title}
        </div>
        <div style={{ fontSize: 11, color: 'var(--text-faint)', marginTop: 2 }}>
          {subtitle}
        </div>
      </div>
      {onRefresh && (
        <button
          type="button"
          onClick={onRefresh}
          style={{
            background: 'transparent',
            border: '1px solid var(--divider-color)',
            borderRadius: 4,
            color: 'var(--text-muted)',
            cursor: 'pointer',
            fontSize: 11,
            padding: '2px 8px',
            flexShrink: 0,
          }}
        >
          Refresh
        </button>
      )}
    </div>
  )
}

function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 10,
        textTransform: 'uppercase',
        letterSpacing: '0.06em',
        color: 'var(--text-faint)',
        margin: '12px 0 4px',
      }}
    >
      {children}
    </div>
  )
}

function ErrorBanner({ message }: { message: string }) {
  return (
    <div
      role="alert"
      style={{
        padding: 8,
        border: '1px solid var(--risk)',
        background: 'var(--risk-soft)',
        color: 'var(--risk)',
        borderRadius: 4,
        fontSize: 11,
        marginBottom: 8,
      }}
    >
      {message}
    </div>
  )
}

function Loading() {
  return (
    <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 8 }}>Loading…</div>
  )
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ color: 'var(--text-muted)', fontSize: 12, lineHeight: 1.6, padding: '12px 4px' }}>
      {children}
    </div>
  )
}

function Footnote({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        marginTop: 12,
        padding: '8px 10px',
        fontSize: 11,
        color: 'var(--text-faint)',
        background: 'var(--background-secondary)',
        border: '1px solid var(--divider-color)',
        borderRadius: 6,
      }}
    >
      {children}
    </div>
  )
}

/** Render an RFC-3339 timestamp as `MM-DD HH:MM` for compactness. */
function formatTimestamp(ts: string): string {
  const date = new Date(ts)
  if (Number.isNaN(date.getTime())) return ts
  const mm = pad(date.getMonth() + 1)
  const dd = pad(date.getDate())
  const HH = pad(date.getHours())
  const MM = pad(date.getMinutes())
  return `${mm}-${dd} ${HH}:${MM}`
}

function pad(n: number): string {
  return n.toString().padStart(2, '0')
}
