import { useWorkflowStore, type RunState, type ValidateState, type WorkflowEntry } from './workflowStore'
import { Icon } from '../../../icons'

interface WorkflowViewProps {
  onRun: (name: string) => void
  onRefresh: () => void
  onValidate: (text: string) => void
}

/**
 * Sidebar listing of `.workflow.toml` definitions in the current
 * forge. Each row exposes a Run button regardless of trigger type —
 * the kernel's `run` handler always honours a manual invocation, the
 * trigger engine just controls automatic firings.
 *
 * Workflows declaring `[inputs]` are run with no caller-supplied
 * variables. The kernel substitutes missing `${inputs.x}` references
 * with empty strings; if a workflow needs a required value, the run
 * surfaces as an error in the row's status pill. A future iteration
 * lifts an inputs-prompt modal in front of the run.
 */
export function WorkflowView({ onRun, onRefresh, onValidate }: WorkflowViewProps) {
  const loading = useWorkflowStore((s) => s.loading)
  const loadError = useWorkflowStore((s) => s.loadError)
  const workflows = useWorkflowStore((s) => s.workflows)
  const runs = useWorkflowStore((s) => s.runs)
  const validate = useWorkflowStore((s) => s.validate)
  const setValidateText = useWorkflowStore((s) => s.setValidateText)
  const resetValidate = useWorkflowStore((s) => s.resetValidate)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--bg)',
        color: 'var(--fg)',
        fontFamily: 'var(--f-ui)',
        fontSize: 'var(--ui-size, 13px)',
      }}
    >
      <Header onRefresh={onRefresh} loading={loading} count={workflows.length} />
      <div style={{ flex: '1 1 auto', overflow: 'auto' }}>
        {loadError ? (
          <Centered colour="var(--risk)">{loadError}</Centered>
        ) : loading && workflows.length === 0 ? (
          <Centered colour="var(--fg-dim)">Loading…</Centered>
        ) : workflows.length === 0 ? (
          <Centered colour="var(--fg-dim)">
            No workflows. Add a <code>.workflow.toml</code> under <code>.workflows/</code>.
          </Centered>
        ) : (
          workflows.map((w) => (
            <WorkflowRow
              key={w.name}
              workflow={w}
              run={runs[w.name]}
              onRun={() => onRun(w.name)}
            />
          ))
        )}
      </div>
      <ValidatePanel
        validate={validate}
        onTextChange={setValidateText}
        onValidate={() => onValidate(validate.text)}
        onClear={resetValidate}
      />
    </div>
  )
}

interface HeaderProps {
  onRefresh: () => void
  loading: boolean
  count: number
}

function Header({ onRefresh, loading, count }: HeaderProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 10px',
        borderBottom: '1px solid var(--line-soft)',
        background: 'var(--bg-raised)',
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
        Workflows {count > 0 ? `(${count})` : ''}
      </span>
      <button
        type="button"
        aria-label="Refresh workflows"
        title="Reload .workflows/"
        onClick={onRefresh}
        disabled={loading}
        onMouseEnter={(e) => {
          if (!loading) (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
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
          cursor: loading ? 'default' : 'pointer',
          borderRadius: 'var(--r)',
          opacity: loading ? 0.5 : 1,
        }}
      >
        <Icon name="refresh" size={14} />
      </button>
    </div>
  )
}

interface WorkflowRowProps {
  workflow: WorkflowEntry
  run: RunState | undefined
  onRun: () => void
}

function WorkflowRow({ workflow, run, onRun }: WorkflowRowProps) {
  const status = run?.status ?? 'idle'
  const isRunning = status === 'running'

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        padding: '8px 10px',
        borderBottom: '1px solid var(--line-soft)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span
          style={{
            flex: '1 1 auto',
            fontWeight: 500,
            color: 'var(--fg)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
          title={workflow.name}
        >
          {workflow.name}
        </span>
        <RunStatusPill status={status} error={run?.error ?? null} />
        <RunButton disabled={isRunning} onClick={onRun} />
      </div>
      {workflow.description ? (
        <div
          style={{
            color: 'var(--fg-dim)',
            fontSize: 12,
            lineHeight: 1.35,
          }}
        >
          {workflow.description}
        </div>
      ) : null}
      <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
        <Chip label={workflow.triggerType} />
        <Chip label={`${workflow.stepCount} step${workflow.stepCount === 1 ? '' : 's'}`} />
        {workflow.hasInputs ? <Chip label="inputs" muted /> : null}
      </div>
    </div>
  )
}

interface ChipProps {
  label: string
  muted?: boolean
}

function Chip({ label, muted }: ChipProps) {
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        padding: '1px 6px',
        borderRadius: 999,
        fontSize: 10,
        fontFamily: 'var(--font-mono, monospace)',
        background: 'var(--bg-raised)',
        color: muted ? 'var(--fg-dim)' : 'var(--fg-muted)',
        border: '1px solid var(--line-soft)',
      }}
    >
      {label}
    </span>
  )
}

interface RunButtonProps {
  disabled: boolean
  onClick: () => void
}

function RunButton({ disabled, onClick }: RunButtonProps) {
  return (
    <button
      type="button"
      aria-label="Run workflow"
      title={disabled ? 'Running…' : 'Run workflow'}
      onClick={onClick}
      disabled={disabled}
      onMouseEnter={(e) => {
        if (!disabled) (e.currentTarget as HTMLButtonElement).style.background = 'var(--accent)'
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-raised)'
      }}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: 24,
        height: 24,
        padding: 0,
        background: 'var(--bg-raised)',
        color: 'var(--fg)',
        border: '1px solid var(--line-soft)',
        cursor: disabled ? 'default' : 'pointer',
        borderRadius: 'var(--r)',
        flex: '0 0 auto',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <Icon name="play" size={11} />
    </button>
  )
}

interface RunStatusPillProps {
  status: 'idle' | 'running' | 'done' | 'error'
  error: string | null
}

/**
 * Tiny status indicator next to the Run button. Colour mirrors the
 * design tokens: --ok (done), --accent (running), --risk (error).
 * The error message is exposed via the `title` attribute — the
 * sidebar is too narrow to render it inline, but the user can hover
 * to read it before retrying.
 */
function RunStatusPill({ status, error }: RunStatusPillProps) {
  if (status === 'idle') return null
  const palette = {
    running: { bg: 'var(--accent)', label: 'Running…' },
    done: { bg: 'var(--ok)', label: 'Done' },
    error: { bg: 'var(--risk)', label: 'Error' },
  }[status]
  return (
    <span
      title={error ?? palette.label}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        padding: '1px 6px',
        borderRadius: 999,
        fontSize: 10,
        background: palette.bg,
        color: 'var(--bg)',
        flex: '0 0 auto',
      }}
    >
      {palette.label}
    </span>
  )
}

interface CenteredProps {
  colour: string
  children: React.ReactNode
}

interface ValidatePanelProps {
  validate: ValidateState
  onTextChange: (text: string) => void
  onValidate: () => void
  onClear: () => void
}

/**
 * Inline workflow-TOML validator. Lives at the bottom of the
 * sidebar — collapsed by default to keep the main list visible —
 * and dispatches `com.nexus.workflow::validate` when the user clicks
 * Validate. The kernel parser's error messages already include line
 * hints from `serde_toml`; we render them verbatim in a monospace
 * block so authors can correlate to their source.
 *
 * Sized for use as a sidebar pane (~280px wide). Textarea height is
 * a fixed ~120px because the surrounding scroll container handles
 * the rest.
 */
function ValidatePanel({ validate, onTextChange, onValidate, onClear }: ValidatePanelProps) {
  const isOpen = validate.text.length > 0 || validate.status !== 'idle'
  return (
    <details
      open={isOpen}
      style={{
        flex: '0 0 auto',
        borderTop: '1px solid var(--line-soft)',
        background: 'var(--bg-raised)',
        fontSize: 12,
      }}
    >
      <summary
        style={{
          listStyle: 'none',
          cursor: 'pointer',
          padding: '6px 10px',
          color: 'var(--fg-muted)',
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          userSelect: 'none',
        }}
      >
        Validate TOML
      </summary>
      <div style={{ padding: '6px 10px 10px', display: 'flex', flexDirection: 'column', gap: 6 }}>
        <textarea
          value={validate.text}
          onChange={(e) => onTextChange(e.target.value)}
          spellCheck={false}
          placeholder={'[workflow]\nname = "Example"\n\n[trigger]\ntype = "manual"'}
          aria-label="Workflow TOML to validate"
          style={{
            width: '100%',
            minHeight: 120,
            resize: 'vertical',
            padding: 6,
            fontFamily: 'var(--font-mono, monospace)',
            fontSize: 11,
            lineHeight: 1.4,
            background: 'var(--bg)',
            color: 'var(--fg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            boxSizing: 'border-box',
          }}
        />
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <button
            type="button"
            onClick={onValidate}
            disabled={validate.status === 'validating'}
            style={{
              padding: '4px 10px',
              background: 'var(--accent)',
              color: 'var(--bg)',
              border: 0,
              borderRadius: 'var(--r)',
              cursor: validate.status === 'validating' ? 'default' : 'pointer',
              fontSize: 11,
              opacity: validate.status === 'validating' ? 0.6 : 1,
            }}
          >
            {validate.status === 'validating' ? 'Validating…' : 'Validate'}
          </button>
          <button
            type="button"
            onClick={onClear}
            disabled={validate.status === 'validating' || (validate.text === '' && validate.status === 'idle')}
            style={{
              padding: '4px 10px',
              background: 'transparent',
              color: 'var(--fg-muted)',
              border: '1px solid var(--line-soft)',
              borderRadius: 'var(--r)',
              cursor: 'pointer',
              fontSize: 11,
            }}
          >
            Clear
          </button>
          <ValidateBadge state={validate} />
        </div>
        <ValidateResult state={validate} />
      </div>
    </details>
  )
}

interface ValidateBadgeProps {
  state: ValidateState
}

function ValidateBadge({ state }: ValidateBadgeProps) {
  if (state.status === 'idle' || state.status === 'validating') return null
  const palette =
    state.status === 'ok'
      ? { bg: 'var(--ok)', label: 'Valid' }
      : { bg: 'var(--risk)', label: 'Invalid' }
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        padding: '1px 6px',
        borderRadius: 999,
        fontSize: 10,
        background: palette.bg,
        color: 'var(--bg)',
      }}
    >
      {palette.label}
    </span>
  )
}

interface ValidateResultProps {
  state: ValidateState
}

/**
 * Bottom slot of the validator. Renders one of three things:
 *  - parser error in a monospace block (preserves serde_toml's
 *    line/col hints exactly as the kernel produced them);
 *  - success line naming the parsed workflow;
 *  - nothing (idle / validating).
 */
function ValidateResult({ state }: ValidateResultProps) {
  if (state.status === 'error' && state.error) {
    return (
      <pre
        style={{
          margin: 0,
          padding: '6px 8px',
          background: 'var(--bg)',
          color: 'var(--risk)',
          border: '1px solid var(--risk)',
          borderRadius: 'var(--r)',
          fontFamily: 'var(--font-mono, monospace)',
          fontSize: 11,
          lineHeight: 1.4,
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
        }}
      >
        {state.error}
      </pre>
    )
  }
  if (state.status === 'ok') {
    const label = state.validatedName
      ? `Parsed workflow "${state.validatedName}".`
      : 'Workflow TOML is valid.'
    return (
      <div
        style={{
          padding: '4px 0',
          color: 'var(--ok)',
          fontSize: 11,
        }}
      >
        {label}
      </div>
    )
  }
  return null
}

function Centered({ colour, children }: CenteredProps) {
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
