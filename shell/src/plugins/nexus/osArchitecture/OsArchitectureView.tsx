// BL-054 Phase 2 — architecture panel view.
//
// Renders the parsed architecture.md as a collapsible domain → task
// tree, with a four-attribute chip row per task and inline drift
// warnings. The view is purely presentational; refresh is driven by
// the parent plugin's `index.ts`.

import { useOsArchitectureStore } from './osArchitectureStore'
import type {
  ArchitectureTask,
  TaskClass,
  TaskMemoryDest,
  TaskType,
} from './architectureParser'
import type { DriftItem } from './driftDetect'
import { taskKey } from './driftDetect'

interface Props {
  onRefresh: () => void
}

export function OsArchitectureView({ onRefresh }: Props) {
  const status = useOsArchitectureStore((s) => s.status)
  const error = useOsArchitectureStore((s) => s.error)
  const architecture = useOsArchitectureStore((s) => s.architecture)
  const drift = useOsArchitectureStore((s) => s.drift)
  const collapsed = useOsArchitectureStore((s) => s.collapsed)
  const toggleCollapsed = useOsArchitectureStore((s) => s.toggleCollapsed)

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
      <Header onRefresh={onRefresh} />
      <div style={{ flex: 1, overflowY: 'auto' }}>
        {status === 'error' && <ErrorState error={error ?? 'Unknown error'} />}
        {status === 'loading' && <LoadingState />}
        {status === 'idle' && <LoadingState />}
        {status === 'missing' && <MissingState />}
        {status === 'ok' && architecture && drift && (
          architecture.domains.length === 0
            ? <EmptyState preamble={architecture.preamble} />
            : (
              <DomainList
                architecture={architecture}
                drift={drift}
                collapsed={collapsed}
                onToggleCollapsed={toggleCollapsed}
              />
            )
        )}
        {status === 'ok' && drift && drift.unattached.length > 0 && (
          <UndocumentedSkills items={drift.unattached} />
        )}
      </div>
    </div>
  )
}

function Header({ onRefresh }: { onRefresh: () => void }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '8px 12px',
        borderBottom: '1px solid var(--background-modifier-border)',
        flexShrink: 0,
      }}
    >
      <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-muted)' }}>
        ARCHITECTURE
      </span>
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
        }}
        title="Re-read architecture.md and refresh drift detection"
      >
        Refresh
      </button>
    </div>
  )
}

function LoadingState() {
  return (
    <div style={{ padding: 16, color: 'var(--text-faint)', fontSize: 13 }}>
      Loading…
    </div>
  )
}

function ErrorState({ error }: { error: string }) {
  return (
    <div style={{ padding: 16, color: 'var(--risk)', fontSize: 13 }}>
      Failed to load architecture.md: {error}
    </div>
  )
}

function MissingState() {
  return (
    <div style={{ padding: 16, color: 'var(--text-muted)', fontSize: 13, lineHeight: 1.6 }}>
      <p style={{ margin: 0 }}>
        No <code>architecture.md</code> found at the forge root.
      </p>
      <p style={{ margin: '8px 0 0' }}>
        Run the BL-054 Phase 5 OS Setup skill (when it ships) or
        scaffold an OS-template forge with{' '}
        <code>nexus forge init --template os</code> to seed one.
      </p>
    </div>
  )
}

function EmptyState({ preamble }: { preamble: string }) {
  return (
    <div style={{ padding: 16, color: 'var(--text-muted)', fontSize: 13, lineHeight: 1.6 }}>
      <p style={{ margin: 0 }}>
        <code>architecture.md</code> exists but no domains are registered yet.
      </p>
      {preamble && (
        <pre
          style={{
            marginTop: 12,
            padding: 12,
            background: 'var(--background-secondary)',
            border: '1px solid var(--divider-color)',
            borderRadius: 6,
            fontFamily: 'var(--font-monospace)',
            fontSize: 11,
            whiteSpace: 'pre-wrap',
            color: 'var(--text-faint)',
          }}
        >
          {preamble}
        </pre>
      )}
    </div>
  )
}

function DomainList({
  architecture,
  drift,
  collapsed,
  onToggleCollapsed,
}: {
  architecture: NonNullable<ReturnType<typeof useOsArchitectureStore.getState>['architecture']>
  drift: NonNullable<ReturnType<typeof useOsArchitectureStore.getState>['drift']>
  collapsed: Set<string>
  onToggleCollapsed: (domain: string) => void
}) {
  return (
    <div style={{ padding: '6px 0 16px' }}>
      {architecture.domains.map((domain) => {
        const isCollapsed = collapsed.has(domain.name)
        return (
          <div key={domain.name} style={{ marginBottom: 12 }}>
            <button
              type="button"
              onClick={() => onToggleCollapsed(domain.name)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                width: '100%',
                padding: '6px 12px',
                background: 'transparent',
                border: 'none',
                color: 'var(--text-normal)',
                cursor: 'pointer',
                fontSize: 13,
                fontWeight: 600,
                textAlign: 'left',
              }}
            >
              <span
                aria-hidden
                style={{
                  display: 'inline-block',
                  transform: isCollapsed ? 'rotate(-90deg)' : 'none',
                  transition: 'transform 100ms ease',
                  fontSize: 10,
                  color: 'var(--text-faint)',
                }}
              >
                ▾
              </span>
              <span>{domain.name}</span>
              <span style={{ marginLeft: 'auto', fontSize: 10, color: 'var(--text-faint)' }}>
                {domain.tasks.length} {domain.tasks.length === 1 ? 'task' : 'tasks'}
              </span>
            </button>
            {!isCollapsed && (
              <div style={{ paddingLeft: 24 }}>
                {domain.tasks.length === 0 ? (
                  <div style={{ padding: '4px 0', color: 'var(--text-faint)', fontSize: 12 }}>
                    No tasks.
                  </div>
                ) : (
                  domain.tasks.map((task) => (
                    <TaskRow
                      key={task.id}
                      task={task}
                      drift={drift.byTask.get(taskKey(domain.name, task.id)) ?? []}
                    />
                  ))
                )}
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}

function TaskRow({ task, drift }: { task: ArchitectureTask; drift: DriftItem[] }) {
  return (
    <div
      style={{
        padding: '6px 12px 8px 0',
        borderBottom: '1px dashed var(--divider-color)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, flexWrap: 'wrap' }}>
        <span
          style={{
            fontFamily: 'var(--font-monospace)',
            fontSize: 12,
            color: 'var(--text-normal)',
          }}
        >
          {task.id}
        </span>
        {task.description && (
          <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
            — {task.description}
          </span>
        )}
      </div>
      <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 4 }}>
        <Chip label={task.type} kind={typeChipKind(task.type)} />
        <Chip label={task.class.toUpperCase()} kind={classChipKind(task.class)} />
        <Chip label={`→ ${task.memoryDest}`} kind={memoryChipKind(task.memoryDest)} />
        <Chip label={task.automation.raw} kind={task.automation.kind === 'none' ? 'muted' : 'accent'} />
      </div>
      {drift.length > 0 && (
        <div style={{ marginTop: 6 }}>
          {drift.map((d) => (
            <div
              key={d.kind}
              style={{
                fontSize: 11,
                color: 'var(--warn)',
                padding: '4px 8px',
                background: 'var(--warn-soft)',
                borderRadius: 4,
                marginTop: 2,
              }}
            >
              ⚠ {d.message}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function UndocumentedSkills({ items }: { items: DriftItem[] }) {
  return (
    <div style={{ padding: '12px 12px 16px' }}>
      <div
        style={{
          fontSize: 11,
          fontWeight: 600,
          textTransform: 'uppercase',
          letterSpacing: '0.06em',
          color: 'var(--text-muted)',
          marginBottom: 6,
        }}
      >
        Undocumented skills
      </div>
      {items.map((d) => (
        <div
          key={d.id}
          style={{
            fontSize: 12,
            padding: '4px 8px',
            color: 'var(--text-muted)',
            background: 'var(--background-secondary)',
            borderRadius: 4,
            marginBottom: 2,
          }}
        >
          {d.message}
        </div>
      ))}
    </div>
  )
}

type ChipKind = 'accent' | 'ok' | 'warn' | 'risk' | 'muted'

function Chip({ label, kind }: { label: string; kind: ChipKind }) {
  const palette: Record<ChipKind, { bg: string; fg: string; border: string }> = {
    accent: {
      bg: 'var(--interactive-accent-soft)',
      fg: 'var(--interactive-accent)',
      border: 'var(--interactive-accent-soft)',
    },
    ok: { bg: 'var(--ok-soft)', fg: 'var(--ok)', border: 'var(--ok-soft)' },
    warn: { bg: 'var(--warn-soft)', fg: 'var(--warn)', border: 'var(--warn-soft)' },
    risk: { bg: 'var(--risk-soft)', fg: 'var(--risk)', border: 'var(--risk-soft)' },
    muted: {
      bg: 'var(--background-secondary)',
      fg: 'var(--text-muted)',
      border: 'var(--divider-color)',
    },
  }
  const p = palette[kind]
  return (
    <span
      style={{
        fontFamily: 'var(--font-monospace)',
        fontSize: 10,
        padding: '1px 6px',
        borderRadius: 999,
        background: p.bg,
        color: p.fg,
        border: `1px solid ${p.border}`,
        whiteSpace: 'nowrap',
      }}
    >
      {label}
    </span>
  )
}

function typeChipKind(t: TaskType): ChipKind {
  return t === 'unknown' ? 'risk' : 'accent'
}

function classChipKind(c: TaskClass): ChipKind {
  if (c === 'foundation') return 'ok'
  if (c === 'capability') return 'muted'
  return 'risk'
}

function memoryChipKind(d: TaskMemoryDest): ChipKind {
  return d === 'unknown' ? 'risk' : 'muted'
}
