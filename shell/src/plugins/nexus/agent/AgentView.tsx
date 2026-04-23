import {
  KNOWN_ARCHETYPES,
  useAgentStore,
  type ArchetypeId,
  type HistoryRow,
  type Plan,
  type PlanStep,
  type RunMode,
  type StepStatus,
} from './agentStore'
import { Icon } from '../../../icons'

interface AgentViewProps {
  onPlan: () => void
  onRun: () => void
  onLoadHistory: (planId: string) => void
  onRefreshHistory: () => void
  onDeleteHistory: (planId: string) => void
  /** Approve the step currently awaiting decision (step-by-step mode). */
  onApproveStep: () => void
  /** Skip the step currently awaiting decision. */
  onSkipStep: () => void
  /** Stop the stepped run; remaining steps marked skipped. */
  onStopRun: () => void
}

/**
 * Pane-mode workspace for `com.nexus.agent`. Two columns:
 *
 *   • Left (240px): persisted run history. Click a row to load that
 *     plan + observation into the right column.
 *   • Right (flex): goal composer at top, then the active plan with
 *     live per-step status (driven by the kernel's
 *     com.nexus.agent.{run_start,step_start,step_done,run_done}
 *     topics — see index.ts), then the final observation if present.
 *
 * Step-by-step approval (HANDLER_EXECUTE_STEP) and the archetype
 * picker are both wired through the composer — see `ModeToggle`
 * (auto vs step) and `ArchetypeSelect` below. Step-mode runs aren't
 * persisted to history because the kernel's `execute_step` handler
 * doesn't save records; that's a documented v1 limitation in
 * `agentStore.ts::RunMode`, not a missing feature.
 */
export function AgentView({
  onPlan,
  onRun,
  onLoadHistory,
  onRefreshHistory,
  onDeleteHistory,
  onApproveStep,
  onSkipStep,
  onStopRun,
}: AgentViewProps) {
  return (
    <div
      style={{
        display: 'flex',
        width: '100%',
        height: '100%',
        background: 'var(--bg)',
        color: 'var(--fg)',
        fontFamily: 'var(--f-ui)',
        fontSize: 'var(--ui-size, 13px)',
      }}
    >
      <HistoryColumn onSelect={onLoadHistory} onRefresh={onRefreshHistory} onDelete={onDeleteHistory} />
      <RunColumn
        onPlan={onPlan}
        onRun={onRun}
        onApproveStep={onApproveStep}
        onSkipStep={onSkipStep}
        onStopRun={onStopRun}
      />
    </div>
  )
}

function HistoryColumn({
  onSelect,
  onRefresh,
  onDelete,
}: {
  onSelect: (planId: string) => void
  onRefresh: () => void
  onDelete: (planId: string) => void
}) {
  const loading = useAgentStore((s) => s.historyLoading)
  const error = useAgentStore((s) => s.historyError)
  const history = useAgentStore((s) => s.history)
  const currentPlanId = useAgentStore((s) => s.plan?.id ?? null)

  return (
    <div
      style={{
        flex: '0 0 240px',
        display: 'flex',
        flexDirection: 'column',
        borderRight: '1px solid var(--line)',
        background: 'var(--bg-raised)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '6px 10px',
          borderBottom: '1px solid var(--line-soft)',
          flex: '0 0 auto',
        }}
      >
        <span
          style={{
            flex: '1 1 auto',
            color: 'var(--fg-muted)',
            fontSize: 11,
            textTransform: 'uppercase',
            letterSpacing: '0.04em',
          }}
        >
          History {history.length > 0 ? `(${history.length})` : ''}
        </span>
        <IconButton title="Refresh history" onClick={onRefresh} disabled={loading} icon="refresh" size={14} />
      </div>
      <div style={{ flex: '1 1 auto', overflow: 'auto' }}>
        {error ? (
          <Centered colour="var(--risk)">{error}</Centered>
        ) : loading && history.length === 0 ? (
          <Centered colour="var(--fg-dim)">Loading…</Centered>
        ) : history.length === 0 ? (
          <Centered colour="var(--fg-dim)">No past runs.</Centered>
        ) : (
          history.map((h) => (
            <HistoryItem
              key={h.plan_id}
              row={h}
              active={h.plan_id === currentPlanId}
              onClick={() => onSelect(h.plan_id)}
              onDelete={() => onDelete(h.plan_id)}
            />
          ))
        )}
      </div>
    </div>
  )
}

function HistoryItem({
  row,
  active,
  onClick,
  onDelete,
}: {
  row: HistoryRow
  active: boolean
  onClick: () => void
  onDelete: () => void
}) {
  const success = row.success === true ? 'ok' : row.success === false ? 'failed' : 'unknown'
  const colour =
    success === 'ok' ? 'var(--ok)' : success === 'failed' ? 'var(--risk)' : 'var(--fg-dim)'
  return (
    <div
      onClick={onClick}
      role="button"
      // The trash button is reveal-on-hover. CSS-only via the
      // group-hover pattern would need a class; here we just render
      // the button at full opacity and let the row's bg-hover do
      // most of the affordance work.
      style={{
        display: 'grid',
        gridTemplateColumns: '1fr auto',
        alignItems: 'center',
        columnGap: 6,
        padding: '6px 10px',
        cursor: 'pointer',
        borderBottom: '1px solid var(--line-soft)',
        background: active ? 'var(--bg-hover)' : 'transparent',
        borderLeft: `2px solid ${active ? 'var(--accent)' : 'transparent'}`,
      }}
      onMouseEnter={(e) => {
        if (!active) (e.currentTarget as HTMLDivElement).style.background = 'var(--bg-hover)'
      }}
      onMouseLeave={(e) => {
        if (!active) (e.currentTarget as HTMLDivElement).style.background = 'transparent'
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ width: 6, height: 6, borderRadius: '50%', background: colour, flex: '0 0 auto' }} />
          <span
            style={{
              flex: '1 1 auto',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              fontSize: 12,
            }}
            title={row.goal ?? row.plan_id}
          >
            {row.goal ?? <em style={{ color: 'var(--fg-dim)' }}>(no goal)</em>}
          </span>
        </div>
        <div
          style={{
            display: 'flex',
            gap: 8,
            fontSize: 10,
            color: 'var(--fg-dim)',
            fontFamily: 'var(--f-mono, monospace)',
          }}
        >
          <span>{row.steps} step{row.steps === 1 ? '' : 's'}</span>
          {row.created_at ? <span>{shortDate(row.created_at)}</span> : null}
        </div>
      </div>
      <button
        type="button"
        aria-label="Delete history entry"
        title="Delete this run from history"
        onClick={(e) => {
          // Stop the row click from firing — we don't want a delete
          // press to also load the (about-to-be-deleted) plan.
          e.stopPropagation()
          onDelete()
        }}
        onMouseEnter={(e) => {
          (e.currentTarget as HTMLButtonElement).style.color = 'var(--risk)'
          ;(e.currentTarget as HTMLButtonElement).style.background = 'var(--bg)'
        }}
        onMouseLeave={(e) => {
          (e.currentTarget as HTMLButtonElement).style.color = 'var(--fg-dim)'
          ;(e.currentTarget as HTMLButtonElement).style.background = 'transparent'
        }}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 22,
          height: 22,
          padding: 0,
          border: 0,
          background: 'transparent',
          color: 'var(--fg-dim)',
          cursor: 'pointer',
          borderRadius: 'var(--r)',
          flex: '0 0 auto',
        }}
      >
        <Icon name="trash" size={12} />
      </button>
    </div>
  )
}

function shortDate(iso: string): string {
  // History timestamps are ISO 8601. Render as YYYY-MM-DD HH:MM in
  // the user's locale tz. Fall back to the raw string on parse error.
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

function RunColumn({
  onPlan,
  onRun,
  onApproveStep,
  onSkipStep,
  onStopRun,
}: {
  onPlan: () => void
  onRun: () => void
  onApproveStep: () => void
  onSkipStep: () => void
  onStopRun: () => void
}) {
  const goal = useAgentStore((s) => s.goal)
  const setGoal = useAgentStore((s) => s.setGoal)
  const runMode = useAgentStore((s) => s.runMode)
  const setRunMode = useAgentStore((s) => s.setRunMode)
  const phase = useAgentStore((s) => s.phase)
  const plan = useAgentStore((s) => s.plan)
  const observation = useAgentStore((s) => s.observation)
  const runError = useAgentStore((s) => s.runError)

  const planning = phase === 'planning'
  const running = phase === 'running'
  // `awaiting` is the step-mode equivalent of running — the current
  // step is in flight or waiting for user approval. Composer stays
  // disabled either way.
  const awaiting = phase === 'awaiting'
  const busy = planning || running || awaiting

  return (
    <div style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div style={{ padding: 16, borderBottom: '1px solid var(--line-soft)', flex: '0 0 auto' }}>
        <div
          style={{
            fontSize: 11,
            textTransform: 'uppercase',
            letterSpacing: '0.04em',
            color: 'var(--fg-muted)',
            marginBottom: 6,
          }}
        >
          Goal
        </div>
        <textarea
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          rows={3}
          placeholder="Describe what the agent should do…"
          spellCheck={false}
          style={{
            width: '100%',
            boxSizing: 'border-box',
            padding: 8,
            background: 'var(--bg-raised)',
            color: 'var(--fg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            font: 'inherit',
            resize: 'vertical',
            outline: 'none',
          }}
        />
        <div style={{ display: 'flex', gap: 8, marginTop: 8, alignItems: 'center' }}>
          <ActionButton
            label={planning ? 'Planning…' : 'Plan'}
            onClick={onPlan}
            disabled={busy || goal.trim() === ''}
            primary={false}
          />
          <ActionButton
            label={running || awaiting ? (runMode === 'step' ? 'Running…' : 'Running…') : 'Run'}
            onClick={onRun}
            disabled={busy || goal.trim() === ''}
            primary
          />
          <ModeToggle value={runMode} onChange={setRunMode} disabled={busy} />
          <ArchetypeSelect disabled={busy} />
          {phase === 'done' && observation ? (
            <span
              style={{
                fontSize: 11,
                color: observation.success ? 'var(--ok)' : 'var(--risk)',
                marginLeft: 'auto',
              }}
            >
              {observation.success ? 'Run finished cleanly.' : 'Run failed — see steps below.'}
            </span>
          ) : null}
        </div>
      </div>

      <div style={{ flex: '1 1 auto', overflow: 'auto', padding: 16 }}>
        {runError ? (
          <div style={{ color: 'var(--risk)', whiteSpace: 'pre-wrap' }}>{runError}</div>
        ) : plan ? (
          <PlanView
            plan={plan}
            onApproveStep={onApproveStep}
            onSkipStep={onSkipStep}
            onStopRun={onStopRun}
          />
        ) : (
          <div style={{ color: 'var(--fg-dim)', textAlign: 'center', marginTop: 32 }}>
            Enter a goal and press Plan to generate steps, or Run to plan + execute in one go.
          </div>
        )}
      </div>
    </div>
  )
}

function ModeToggle({
  value,
  onChange,
  disabled,
}: {
  value: RunMode
  onChange: (m: RunMode) => void
  disabled: boolean
}) {
  return (
    <div
      role="group"
      aria-label="Run mode"
      title="Auto = run all steps. Step-by-step = approve each step (no history persistence)."
      style={{
        display: 'inline-flex',
        background: 'var(--bg-raised)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        overflow: 'hidden',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {(['auto', 'step'] as const).map((m) => (
        <button
          key={m}
          type="button"
          onClick={() => onChange(m)}
          disabled={disabled}
          style={{
            padding: '4px 8px',
            background: value === m ? 'var(--accent)' : 'transparent',
            color: value === m ? 'var(--bg)' : 'var(--fg-muted)',
            border: 0,
            font: 'inherit',
            fontSize: 11,
            cursor: disabled ? 'default' : 'pointer',
          }}
        >
          {m === 'auto' ? 'Auto' : 'Step'}
        </button>
      ))}
    </div>
  )
}

/**
 * Tiny archetype dropdown — forwards to `com.nexus.agent::{plan,run}`
 * as the optional `archetype` arg. The kernel resolves the string
 * case-insensitively (see crates/nexus-agent/src/archetypes.rs); an
 * unknown value falls back to the default planner. The list is
 * hardcoded against `KNOWN_ARCHETYPES`; once the kernel exposes a
 * `list_archetypes` IPC the catalogue should be fetched at startup.
 */
function ArchetypeSelect({ disabled }: { disabled: boolean }) {
  const archetype = useAgentStore((s) => s.archetype)
  const setArchetype = useAgentStore((s) => s.setArchetype)
  return (
    <select
      aria-label="Planner archetype"
      title="Pick a planner archetype to bias the LLM toward a domain (writer, coder, researcher) — Default uses the general planner."
      value={archetype ?? ''}
      disabled={disabled}
      onChange={(e) => {
        const v = e.target.value
        setArchetype(v === '' ? null : (v as ArchetypeId))
      }}
      style={{
        padding: '4px 6px',
        background: 'var(--bg-raised)',
        color: 'var(--fg)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        font: 'inherit',
        fontSize: 11,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <option value="">Default</option>
      {KNOWN_ARCHETYPES.map((a) => (
        <option key={a.id} value={a.id} title={a.description}>
          {a.label}
        </option>
      ))}
    </select>
  )
}

function PlanView({
  plan,
  onApproveStep,
  onSkipStep,
  onStopRun,
}: {
  plan: Plan
  onApproveStep: () => void
  onSkipStep: () => void
  onStopRun: () => void
}) {
  const stepRuntime = useAgentStore((s) => s.stepRuntime)
  const observation = useAgentStore((s) => s.observation)
  const pendingApprovalIndex = useAgentStore((s) => s.pendingApprovalIndex)
  const phase = useAgentStore((s) => s.phase)

  // Build a per-step view-model: prefer the live runtime status, but
  // fall back to the observation's per-step status when a run has
  // completed (the topic stream is post-only so a late observation
  // overrides a stale runtime entry on history-load).
  const observationByStep = new Map(observation?.steps.map((r) => [r.step_id, r]) ?? [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
        <span
          style={{
            fontSize: 11,
            textTransform: 'uppercase',
            letterSpacing: '0.04em',
            color: 'var(--fg-muted)',
          }}
        >
          Plan
        </span>
        <span style={{ color: 'var(--fg-dim)', fontSize: 11, fontFamily: 'var(--f-mono, monospace)' }}>
          {plan.id}
        </span>
        {phase === 'awaiting' ? (
          <span style={{ color: 'var(--accent)', fontSize: 11 }}>Awaiting approval…</span>
        ) : null}
      </div>
      <div style={{ color: 'var(--fg)', fontSize: 13, lineHeight: 1.45 }}>{plan.goal}</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginTop: 4 }}>
        {plan.steps.map((step, idx) => {
          const runtime = stepRuntime[step.id] ?? { status: 'queued' as StepStatus, error: null }
          const obsRow = observationByStep.get(step.id)
          const finalStatus = obsRow?.status
          const status: StepStatus = finalStatus ?? runtime.status
          const error = runtime.error
          // Prefer the live runtime response (captured during step-mode
          // execution); fall back to the observation's response (set
          // when a history row is loaded into the right column).
          const response = runtime.response !== undefined ? runtime.response : obsRow?.response
          const awaitingThis = phase === 'awaiting' && pendingApprovalIndex === idx
          return (
            <StepRow
              key={step.id}
              step={step}
              index={idx}
              status={status}
              error={error}
              response={response}
              awaitingApproval={awaitingThis}
              onApprove={awaitingThis ? onApproveStep : undefined}
              onSkip={awaitingThis ? onSkipStep : undefined}
              onStop={awaitingThis ? onStopRun : undefined}
            />
          )
        })}
      </div>
    </div>
  )
}

function StepRow({
  step,
  index,
  status,
  error,
  response,
  awaitingApproval,
  onApprove,
  onSkip,
  onStop,
}: {
  step: PlanStep
  index: number
  status: StepStatus
  error: string | null
  /**
   * Raw tool-call return value for this step. `undefined` when the
   * step hasn't run yet or was informational (no tool call). When
   * present, rendered via a truncated <pre> block — matches the
   * legacy AgentHistoryPanel.tsx render.
   */
  response: unknown
  awaitingApproval: boolean
  onApprove?: () => void
  onSkip?: () => void
  onStop?: () => void
}) {
  const palette = STATUS_PALETTE[status]
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '24px 1fr auto',
        gap: 8,
        padding: '8px 10px',
        background: awaitingApproval ? 'var(--bg)' : 'var(--bg-raised)',
        border: `1px solid ${awaitingApproval ? 'var(--accent)' : 'var(--line-soft)'}`,
        borderRadius: 'var(--r)',
      }}
    >
      <span
        style={{
          fontFamily: 'var(--f-mono, monospace)',
          fontSize: 11,
          color: 'var(--fg-dim)',
          textAlign: 'right',
          paddingTop: 1,
        }}
      >
        {index + 1}.
      </span>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
        <div style={{ color: 'var(--fg)', fontSize: 13, lineHeight: 1.4 }}>{step.description}</div>
        {step.tool_call ? (
          <div
            style={{
              display: 'flex',
              gap: 6,
              alignItems: 'center',
              fontSize: 11,
              color: 'var(--fg-dim)',
              fontFamily: 'var(--f-mono, monospace)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
            title={`${step.tool_call.target_plugin_id}::${step.tool_call.command_id}`}
          >
            <span style={{ color: 'var(--fg-muted)' }}>{step.tool_call.target_plugin_id}</span>
            <span>·</span>
            <span>{step.tool_call.command_id}</span>
          </div>
        ) : (
          <div style={{ fontSize: 11, color: 'var(--fg-dim)', fontStyle: 'italic' }}>
            informational — no tool call
          </div>
        )}
        {error ? (
          <div style={{ fontSize: 11, color: 'var(--risk)', lineHeight: 1.35 }}>{error}</div>
        ) : null}
        {response !== undefined && response !== null ? (
          <pre
            title="Tool-call response (truncated at 500 chars)"
            style={{
              margin: '4px 0 0',
              padding: 8,
              background: 'var(--bg)',
              border: '1px solid var(--line-soft)',
              borderRadius: 'var(--r)',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              fontFamily: 'var(--f-mono, monospace)',
              fontSize: 11,
              color: 'var(--fg-muted)',
              maxHeight: 240,
              overflow: 'auto',
            }}
          >
            {previewJson(response)}
          </pre>
        ) : null}
      </div>
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 6,
          alignItems: 'flex-end',
          alignSelf: 'flex-start',
        }}
      >
        <span
          title={palette.label}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            padding: '1px 6px',
            borderRadius: 999,
            fontSize: 10,
            background: palette.bg,
            color: palette.fg,
            border: palette.border ? '1px solid var(--line-soft)' : 'none',
            flex: '0 0 auto',
          }}
        >
          {palette.label}
        </span>
        {awaitingApproval && onApprove && onSkip && onStop ? (
          <ApprovalCluster onApprove={onApprove} onSkip={onSkip} onStop={onStop} />
        ) : null}
      </div>
    </div>
  )
}

function ApprovalCluster({
  onApprove,
  onSkip,
  onStop,
}: {
  onApprove: () => void
  onSkip: () => void
  onStop: () => void
}) {
  return (
    <div style={{ display: 'flex', gap: 4 }}>
      <SmallButton label="Approve" tone="primary" onClick={onApprove} />
      <SmallButton label="Skip" tone="neutral" onClick={onSkip} />
      <SmallButton label="Stop" tone="danger" onClick={onStop} />
    </div>
  )
}

function SmallButton({
  label,
  tone,
  onClick,
}: {
  label: string
  tone: 'primary' | 'neutral' | 'danger'
  onClick: () => void
}) {
  const styles =
    tone === 'primary'
      ? { bg: 'var(--accent)', fg: 'var(--bg)', border: 'none' }
      : tone === 'danger'
        ? { bg: 'var(--bg-raised)', fg: 'var(--risk)', border: '1px solid var(--risk)' }
        : { bg: 'var(--bg-raised)', fg: 'var(--fg-muted)', border: '1px solid var(--line-soft)' }
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        padding: '2px 8px',
        background: styles.bg,
        color: styles.fg,
        border: styles.border,
        borderRadius: 'var(--r)',
        font: 'inherit',
        fontSize: 10,
        cursor: 'pointer',
      }}
    >
      {label}
    </button>
  )
}

/**
 * Pretty-print a tool-call response with a hard truncation. Mirrors
 * legacy AgentHistoryPanel.tsx::previewJson but bumps the cap to 500
 * (the legacy default was 400). Strings pass through unchanged so a
 * raw text response doesn't end up double-quoted.
 */
function previewJson(value: unknown, max = 500): string {
  const text = typeof value === 'string' ? value : JSON.stringify(value, null, 2)
  if (text === undefined) return ''
  if (text.length <= max) return text
  return `${text.slice(0, max)}…`
}

const STATUS_PALETTE: Record<StepStatus, { bg: string; fg: string; label: string; border?: boolean }> = {
  queued: { bg: 'var(--bg)', fg: 'var(--fg-dim)', label: 'queued', border: true },
  running: { bg: 'var(--accent)', fg: 'var(--bg)', label: 'running' },
  ok: { bg: 'var(--ok)', fg: 'var(--bg)', label: 'ok' },
  failed: { bg: 'var(--risk)', fg: 'var(--bg)', label: 'failed' },
  skipped: { bg: 'var(--bg)', fg: 'var(--fg-dim)', label: 'skipped', border: true },
}

function IconButton({
  title,
  onClick,
  disabled,
  icon,
  size,
}: {
  title: string
  onClick: () => void
  disabled?: boolean
  icon: 'refresh'
  size: number
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      disabled={disabled}
      onMouseEnter={(e) => {
        if (!disabled) (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'transparent'
      }}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: 24,
        height: 24,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--fg-muted)',
        cursor: disabled ? 'default' : 'pointer',
        borderRadius: 'var(--r)',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <Icon name={icon} size={size} />
    </button>
  )
}

function ActionButton({
  label,
  onClick,
  disabled,
  primary,
}: {
  label: string
  onClick: () => void
  disabled?: boolean
  primary: boolean
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: '6px 14px',
        background: primary ? 'var(--accent)' : 'var(--bg-raised)',
        color: primary ? 'var(--bg)' : 'var(--fg)',
        border: primary ? 'none' : '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        font: 'inherit',
        fontWeight: 500,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {label}
    </button>
  )
}

function Centered({ colour, children }: { colour: string; children: React.ReactNode }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        height: '100%',
        padding: 16,
        textAlign: 'center',
        color: colour,
        fontSize: 12,
        lineHeight: 1.4,
      }}
    >
      {children}
    </div>
  )
}
