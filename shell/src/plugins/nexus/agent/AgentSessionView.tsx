/**
 * Pane-mode view for `nexus.agent`. Drives a single
 * `com.nexus.agent::session_run` lifecycle:
 *
 *   composer → run → live transcript with inline approval card →
 *   completed transcript + sidebar of past sessions.
 *
 * The view is a thin reader of [`useAgentSessionStore`]; every
 * mutation flows through callbacks the runtime in `index.ts`
 * supplies. Keeping the React layer presentation-only is what
 * lets the runtime be unit-tested in isolation.
 */

import { useEffect, useMemo, useState } from 'react'

// Cross-plugin import: same renderer the chat view uses, so the
// agent transcript's markdown gets the same heading hierarchy /
// code blocks / lists treatment instead of rendering as raw
// `**asterisks**`.
import { renderMarkdown } from '../editor/markdownRender'
import './agent.css'
import {
  classifyToolCall,
  riskLabel,
  type RiskLevel,
} from './riskClassifier'
import {
  diffLines,
  DIFF_MAX_LINES,
  extractWriteFileArgs,
  type DiffLine,
} from './diffPreview'
import {
  useAgentSessionStore,
  type ArchetypeInfo,
  type AskAnswerDraft,
  type AskQuestion,
  type PendingAsk,
  type PendingRound,
  type RoundRecord,
  type SessionSummary,
  type SessionTranscript,
  type StepPolicy,
  type ToolCallProposal,
} from './sessionStore'

export interface AgentSessionViewProps {
  onRun(): void
  onApprove(decision: 'approve_all' | 'partial' | 'abort', reason?: string): void
  /** Submit the drafted answers for the open interactive `ask` prompt. */
  onRespondAsk(): void
  onSelectSession(id: string): void
  onDeleteSession(id: string): void
  onRefreshSessions(): void
  onClearLive(): void
  /** AIG-02 — used by the write_file diff preview. Returns `null`
   *  when the file does not yet exist or storage IPC fails. */
  readFile?(path: string): Promise<string | null>
}

export function AgentSessionView(props: AgentSessionViewProps): JSX.Element {
  const goal = useAgentSessionStore((s) => s.goal)
  const archetype = useAgentSessionStore((s) => s.archetype)
  const archetypes = useAgentSessionStore((s) => s.archetypes)
  const stepPolicy = useAgentSessionStore((s) => s.stepPolicy)
  const phase = useAgentSessionStore((s) => s.phase)
  const liveTranscript = useAgentSessionStore((s) => s.liveTranscript)
  const pendingRound = useAgentSessionStore((s) => s.pendingRound)
  const pendingAsk = useAgentSessionStore((s) => s.pendingAsk)
  const liveError = useAgentSessionStore((s) => s.liveError)
  const sessions = useAgentSessionStore((s) => s.sessions)
  const sessionsLoading = useAgentSessionStore((s) => s.sessionsLoading)
  const sessionsError = useAgentSessionStore((s) => s.sessionsError)
  const selectedSessionId = useAgentSessionStore((s) => s.selectedSessionId)
  const selectedTranscript = useAgentSessionStore((s) => s.selectedTranscript)
  const selectedTranscriptError = useAgentSessionStore((s) => s.selectedTranscriptError)
  const setGoal = useAgentSessionStore((s) => s.setGoal)
  const setArchetype = useAgentSessionStore((s) => s.setArchetype)
  const setStepPolicy = useAgentSessionStore((s) => s.setStepPolicy)

  const running = phase === 'starting' || phase === 'awaiting_round' || phase === 'awaiting_approval'
  const canRun = goal.trim().length > 0 && !running

  return (
    <div className="agent-session" data-testid="nexus-agent-session">
      <header className="agent-session__composer">
        <label className="agent-session__field">
          <span>Goal</span>
          <textarea
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            disabled={running}
            placeholder="What should the agent do?"
            rows={3}
            data-testid="agent-goal-input"
          />
        </label>
        <div className="agent-session__row">
          <label className="agent-session__field">
            <span>Archetype</span>
            <ArchetypePicker
              value={archetype}
              archetypes={archetypes}
              onChange={setArchetype}
              disabled={running}
            />
          </label>
          <label className="agent-session__field">
            <span>Approvals</span>
            <PolicyPicker
              value={stepPolicy}
              onChange={setStepPolicy}
              disabled={running}
            />
          </label>
          <button
            type="button"
            className="agent-session__run"
            disabled={!canRun}
            onClick={props.onRun}
            data-testid="agent-run-button"
          >
            {running ? 'Running…' : 'Run'}
          </button>
        </div>
        {liveError ? (
          <p className="agent-session__error" role="alert">
            {liveError}
          </p>
        ) : null}
      </header>

      <section className="agent-session__transcript" aria-label="Session transcript">
        {liveTranscript.length === 0 && !pendingRound && !pendingAsk && phase === 'idle' ? (
          <Empty />
        ) : (
          <>
            {liveTranscript.map((r) => (
              <RoundRecordCard key={`r-${r.round}`} record={r} />
            ))}
            {pendingRound ? (
              <ApprovalCard
                round={pendingRound}
                onSubmit={props.onApprove}
                onApproveRest={() => {
                  // AIG-02 — flip the policy to auto_approve so the
                  // remaining rounds dispatch without prompting, then
                  // submit the current round as approve_all.
                  setStepPolicy('auto_approve')
                  props.onApprove('approve_all')
                }}
                readFile={props.readFile}
              />
            ) : null}
            {pendingAsk ? <AskCard ask={pendingAsk} onSubmit={props.onRespondAsk} /> : null}
            {phase === 'completed' || phase === 'errored' ? (
              <button
                type="button"
                className="agent-session__clear"
                onClick={props.onClearLive}
                data-testid="agent-clear-live"
              >
                New session
              </button>
            ) : null}
          </>
        )}
      </section>

      <aside className="agent-session__history" aria-label="Past sessions">
        <header>
          <h3>Past sessions</h3>
          <button
            type="button"
            onClick={props.onRefreshSessions}
            disabled={sessionsLoading}
            data-testid="agent-history-refresh"
          >
            {sessionsLoading ? '…' : 'Refresh'}
          </button>
        </header>
        {sessionsError ? <p className="agent-session__error">{sessionsError}</p> : null}
        <SessionList
          sessions={sessions}
          selectedId={selectedSessionId}
          onSelect={props.onSelectSession}
          onDelete={props.onDeleteSession}
        />
        {selectedSessionId ? (
          <SelectedTranscript
            transcript={selectedTranscript}
            error={selectedTranscriptError}
          />
        ) : null}
      </aside>
    </div>
  )
}

function Empty(): JSX.Element {
  return (
    <p className="agent-session__empty">
      Enter a goal above and press Run. Approvals appear inline as the
      session progresses.
    </p>
  )
}

interface PolicyPickerProps {
  value: StepPolicy
  onChange(value: StepPolicy): void
  disabled: boolean
}

const POLICY_OPTIONS: ReadonlyArray<{ value: StepPolicy; label: string; title: string }> = [
  {
    value: 'always_ask',
    label: 'Always ask',
    title: 'Pause on every round and require approval before any tool runs.',
  },
  {
    value: 'ask_on_risky',
    label: 'Ask on risky',
    title:
      'Auto-approve rounds that only read; pause when a tool writes, runs a process, or hits the network.',
  },
  {
    value: 'auto_approve',
    label: 'Auto-approve',
    title: 'Run every round without asking. Use only for trusted goals.',
  },
]

function PolicyPicker({ value, onChange, disabled }: PolicyPickerProps): JSX.Element {
  const selected = POLICY_OPTIONS.find((o) => o.value === value) ?? POLICY_OPTIONS[1]
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value as StepPolicy)}
      disabled={disabled}
      title={selected.title}
      data-testid="agent-policy-picker"
    >
      {POLICY_OPTIONS.map((o) => (
        <option key={o.value} value={o.value} title={o.title}>
          {o.label}
        </option>
      ))}
    </select>
  )
}

interface ArchetypePickerProps {
  value: string | null
  archetypes: ArchetypeInfo[]
  onChange(value: string | null): void
  disabled: boolean
}

function ArchetypePicker({ value, archetypes, onChange, disabled }: ArchetypePickerProps): JSX.Element {
  return (
    <select
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value === '' ? null : e.target.value)}
      disabled={disabled}
      data-testid="agent-archetype-picker"
    >
      <option value="">Default planner</option>
      {archetypes.map((a) => (
        <option key={a.id} value={a.id}>
          {a.label}
        </option>
      ))}
    </select>
  )
}

interface RoundRecordCardProps {
  record: RoundRecord
}

function RoundRecordCard({ record }: RoundRecordCardProps): JSX.Element {
  return (
    <article className="agent-round" data-round={record.round}>
      <header>Round {record.round}</header>
      {record.text ? <Narration source={record.text} className="agent-round__text" /> : null}
      {record.tool_calls.length === 0 ? null : (
        <ul className="agent-round__calls">
          {record.tool_calls.map((tc) => (
            <li
              key={tc.id}
              className={`agent-round__call agent-round__call--${
                tc.error ? 'errored' : tc.approved ? 'ok' : 'denied'
              }`}
            >
              <span className="agent-round__call-marker" aria-hidden="true">
                {tc.error ? '✗' : tc.approved ? '✓' : '·'}
              </span>
              <code>{tc.name}</code>
              {tc.error ? (
                <span className="agent-round__call-error"> — {tc.error}</span>
              ) : null}
              {!tc.error && tc.response !== null && tc.response !== undefined ? (
                <pre className="agent-round__call-response">{previewJson(tc.response)}</pre>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </article>
  )
}

interface ApprovalCardProps {
  round: PendingRound
  onSubmit(decision: 'approve_all' | 'partial' | 'abort', reason?: string): void
  onApproveRest(): void
  readFile?(path: string): Promise<string | null>
}

function ApprovalCard({
  round,
  onSubmit,
  onApproveRest,
  readFile,
}: ApprovalCardProps): JSX.Element {
  const toggleApproval = useAgentSessionStore((s) => s.toggleApproval)
  const [abortReason, setAbortReason] = useState<string | null>(null)
  const allApproved = round.toolCalls.every((tc) => round.approvals[tc.id] === true)
  const anyApproved = round.toolCalls.some((tc) => round.approvals[tc.id] === true)

  const submitAbort = () => {
    onSubmit('abort', (abortReason ?? 'user cancelled').trim() || 'user cancelled')
    setAbortReason(null)
  }

  return (
    <article className="agent-approval" data-testid="agent-approval-card">
      <header>
        <span className="agent-approval__badge">Pending approval</span>
        <span className="agent-approval__round">Round {round.round}</span>
      </header>
      {round.text ? <Narration source={round.text} className="agent-approval__text" /> : null}
      <ul className="agent-approval__calls">
        {round.toolCalls.map((tc) => (
          <ToolCallRow
            key={tc.id}
            call={tc}
            approved={round.approvals[tc.id] === true}
            onToggle={(approve) => toggleApproval(tc.id, approve)}
            readFile={readFile}
          />
        ))}
      </ul>
      {abortReason === null ? (
        <footer className="agent-approval__actions">
          <button
            type="button"
            onClick={() => onSubmit(allApproved ? 'approve_all' : 'partial')}
            disabled={!anyApproved}
            data-testid="agent-approval-submit"
          >
            {allApproved ? 'Approve' : 'Approve selected'}
          </button>
          <button
            type="button"
            onClick={onApproveRest}
            disabled={!allApproved}
            title="Approve this round and run the rest of the session without further prompts."
            data-testid="agent-approval-approve-rest"
          >
            Approve &amp; continue
          </button>
          <button
            type="button"
            onClick={() => setAbortReason('')}
            data-testid="agent-approval-abort"
          >
            Reject
          </button>
        </footer>
      ) : (
        <form
          className="agent-approval__abort"
          onSubmit={(e) => {
            e.preventDefault()
            submitAbort()
          }}
        >
          <label className="agent-approval__abort-label" htmlFor="agent-abort-reason">
            Reason for aborting (optional)
          </label>
          <input
            id="agent-abort-reason"
            type="text"
            autoFocus
            value={abortReason}
            placeholder="user cancelled"
            onChange={(e) => setAbortReason(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.preventDefault()
                setAbortReason(null)
              }
            }}
            data-testid="agent-abort-reason-input"
          />
          <div className="agent-approval__actions">
            <button type="submit" data-testid="agent-abort-confirm">
              Confirm abort
            </button>
            <button
              type="button"
              onClick={() => setAbortReason(null)}
              data-testid="agent-abort-cancel"
            >
              Cancel
            </button>
          </div>
        </form>
      )}
    </article>
  )
}

interface AskCardProps {
  ask: PendingAsk
  onSubmit(): void
}

/**
 * Inline panel for an interactive `ask` prompt. Mirrors
 * [`ApprovalCard`] — renders one row per question (radio / checkbox /
 * free-text by shape) and submits the drafted answers. Answers are held
 * in the store so the runtime can read them when posting `ask_respond`.
 */
function AskCard({ ask, onSubmit }: AskCardProps): JSX.Element {
  const updateAskAnswer = useAgentSessionStore((s) => s.updateAskAnswer)
  return (
    <article className="agent-ask" data-testid="agent-ask-card">
      <header>
        <span className="agent-ask__badge">Question</span>
      </header>
      <ul className="agent-ask__questions">
        {ask.questions.map((q) => (
          <AskQuestionRow
            key={q.id}
            question={q}
            draft={ask.answers[q.id]}
            onChange={(patch) => updateAskAnswer(q.id, patch)}
          />
        ))}
      </ul>
      <footer className="agent-ask__actions">
        <button type="button" onClick={onSubmit} data-testid="agent-ask-submit">
          Submit answers
        </button>
      </footer>
    </article>
  )
}

interface AskQuestionRowProps {
  question: AskQuestion
  draft: AskAnswerDraft | undefined
  onChange(patch: Partial<AskAnswerDraft>): void
}

function AskQuestionRow({ question, draft, onChange }: AskQuestionRowProps): JSX.Element {
  const selected = draft?.selected ?? []
  const customInput = draft?.customInput ?? ''
  return (
    <li className="agent-ask__question">
      <p className="agent-ask__prompt">{question.prompt}</p>
      {question.options.length === 0 ? (
        <input
          type="text"
          className="agent-ask__text"
          value={customInput}
          placeholder="Type your answer…"
          onChange={(e) => onChange({ customInput: e.target.value })}
          data-testid={`agent-ask-input-${question.id}`}
        />
      ) : (
        <div className="agent-ask__options">
          {question.options.map((opt) => (
            <label key={opt} className="agent-ask__option">
              <input
                type={question.multi ? 'checkbox' : 'radio'}
                name={`agent-ask-${question.id}`}
                checked={selected.includes(opt)}
                onChange={(e) => {
                  if (question.multi) {
                    const next = e.target.checked
                      ? [...selected, opt]
                      : selected.filter((o) => o !== opt)
                    onChange({ selected: next })
                  } else {
                    onChange({ selected: [opt] })
                  }
                }}
                data-testid={`agent-ask-option-${question.id}-${opt}`}
              />
              <span>{opt}</span>
            </label>
          ))}
        </div>
      )}
    </li>
  )
}

interface ToolCallRowProps {
  call: ToolCallProposal
  approved: boolean
  onToggle(approve: boolean): void
  readFile?(path: string): Promise<string | null>
}

function ToolCallRow({ call, approved, onToggle, readFile }: ToolCallRowProps): JSX.Element {
  const risk = useMemo(
    () => classifyToolCall(call.target_plugin_id, call.command_id),
    [call.target_plugin_id, call.command_id],
  )
  const writeFileArgs = useMemo(() => extractWriteFileArgs(call.args), [call.args])
  const argsPreview = useMemo(() => previewJson(call.args), [call.args])

  return (
    <li
      className={`agent-approval__call agent-approval__call--${approved ? 'ok' : 'denied'} agent-approval__call--risk-${risk}`}
    >
      <label>
        <input
          type="checkbox"
          checked={approved}
          onChange={(e) => onToggle(e.target.checked)}
          data-testid={`agent-approval-toggle-${call.id}`}
        />
        <code>{call.name}</code>
        <RiskBadge level={risk} />
        <span className="agent-approval__target">
          {call.target_plugin_id}::{call.command_id}
        </span>
      </label>
      {writeFileArgs && readFile ? (
        <WriteFileDiff
          path={writeFileArgs.path}
          contents={writeFileArgs.contents}
          readFile={readFile}
        />
      ) : (
        <pre className="agent-approval__args">{argsPreview}</pre>
      )}
    </li>
  )
}

function RiskBadge({ level }: { level: RiskLevel }): JSX.Element {
  return (
    <span
      className={`agent-approval__risk agent-approval__risk--${level}`}
      title={`Risk: ${level}`}
      data-testid={`agent-risk-${level}`}
    >
      {riskLabel(level)}
    </span>
  )
}

interface WriteFileDiffProps {
  path: string
  contents: string
  readFile(path: string): Promise<string | null>
}

/**
 * Loads the current on-disk contents and renders a unified-style
 * line diff against the model's proposed contents. Falls back to
 * showing the full proposed body when the file does not yet exist
 * (`null` from readFile).
 */
function WriteFileDiff({ path, contents, readFile }: WriteFileDiffProps): JSX.Element {
  const [before, setBefore] = useState<string | null | undefined>(undefined)
  useEffect(() => {
    let cancelled = false
    void readFile(path).then((b) => {
      if (!cancelled) setBefore(b)
    })
    return () => {
      cancelled = true
    }
  }, [path, readFile])

  if (before === undefined) {
    return (
      <div className="agent-approval__diff agent-approval__diff--loading">
        <span className="agent-approval__diff-path">{path}</span>
        <span className="agent-approval__diff-status">loading…</span>
      </div>
    )
  }

  if (before === null) {
    return (
      <div className="agent-approval__diff">
        <header>
          <span className="agent-approval__diff-path">{path}</span>
          <span className="agent-approval__diff-status">new file</span>
        </header>
        <pre className="agent-approval__diff-body">
          {contents.length > 4000 ? contents.slice(0, 4000) + '\n…' : contents}
        </pre>
      </div>
    )
  }

  const diff = diffLines(before, contents)
  if (diff.unchanged) {
    return (
      <div className="agent-approval__diff">
        <header>
          <span className="agent-approval__diff-path">{path}</span>
          <span className="agent-approval__diff-status">no changes</span>
        </header>
      </div>
    )
  }

  return (
    <div className="agent-approval__diff">
      <header>
        <span className="agent-approval__diff-path">{path}</span>
        <span className="agent-approval__diff-status">
          {diff.lines.filter((l) => l.kind === 'add').length} added,{' '}
          {diff.lines.filter((l) => l.kind === 'remove').length} removed
        </span>
      </header>
      <pre className="agent-approval__diff-body">
        {diff.lines.map((l, i) => (
          <DiffLineRow key={i} line={l} />
        ))}
      </pre>
      {diff.truncated ? (
        <footer className="agent-approval__diff-truncated">
          Diff truncated at {DIFF_MAX_LINES} lines.
        </footer>
      ) : null}
    </div>
  )
}

function DiffLineRow({ line }: { line: DiffLine }): JSX.Element {
  const sigil = line.kind === 'add' ? '+' : line.kind === 'remove' ? '-' : ' '
  return (
    <span className={`agent-approval__diff-line agent-approval__diff-line--${line.kind}`}>
      {sigil}{' '}
      {line.segments
        ? line.segments.map((seg, i) =>
            seg.kind === 'common' ? (
              <span key={i}>{seg.text}</span>
            ) : (
              <span
                key={i}
                className={`agent-approval__diff-word agent-approval__diff-word--${seg.kind}`}
              >
                {seg.text}
              </span>
            ),
          )
        : line.text}
      {'\n'}
    </span>
  )
}

interface SessionListProps {
  sessions: SessionSummary[]
  selectedId: string | null
  onSelect(id: string): void
  onDelete(id: string): void
}

function SessionList({ sessions, selectedId, onSelect, onDelete }: SessionListProps): JSX.Element {
  if (sessions.length === 0) {
    return <p className="agent-session__empty-history">No past sessions yet.</p>
  }
  return (
    <ul className="agent-session__history-list">
      {sessions.map((s) => (
        <li
          key={s.id}
          className={`agent-session__history-row ${
            selectedId === s.id ? 'agent-session__history-row--selected' : ''
          }`}
        >
          <button type="button" onClick={() => onSelect(s.id)}>
            <span className="agent-session__history-goal">{s.goal || '(no goal)'}</span>
            <OutcomePill outcome={s.outcome} />
            <span className="agent-session__history-time">{formatTimestamp(s.started_at)}</span>
          </button>
          <button
            type="button"
            className="agent-session__history-delete"
            aria-label={`Delete session ${s.id}`}
            onClick={() => onDelete(s.id)}
          >
            ×
          </button>
        </li>
      ))}
    </ul>
  )
}

interface SelectedTranscriptProps {
  transcript: SessionTranscript | null
  error: string | null
}

function SelectedTranscript({ transcript, error }: SelectedTranscriptProps): JSX.Element {
  if (error) return <p className="agent-session__error">{error}</p>
  if (!transcript) return <p className="agent-session__empty">Loading…</p>
  return (
    <div className="agent-session__selected" data-testid="agent-selected-transcript">
      <header>
        <strong>{transcript.goal}</strong>
        <OutcomePill outcome={transcript.outcome} />
      </header>
      {transcript.rounds.map((r) => (
        <RoundRecordCard key={`sel-${r.round}`} record={r} />
      ))}
    </div>
  )
}

// ── helpers ────────────────────────────────────────────────────────────

interface NarrationProps {
  source: string
  className: string
}

/**
 * Render a model narration string as markdown — same renderer the
 * chat view uses so headings, lists, code blocks, and inline marks
 * (`**bold**`, `` `code` ``) display properly. The transcript text
 * the model emits is markdown by convention; rendering it raw makes
 * agent runs much harder to skim.
 */
function Narration({ source, className }: NarrationProps): JSX.Element {
  const html = useMemo(() => renderMarkdown(source), [source])
  return (
    <div
      className={`${className} nexus-markdown-body`}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}

const KNOWN_OUTCOMES = new Set([
  'complete',
  'aborted',
  'errored',
  'max_rounds',
  'approval_timeout',
])

interface OutcomePillProps {
  outcome: string | undefined | null
}

/**
 * Always render the outcome chip — degrades gracefully when the
 * underlying record has a missing or unrecognised outcome (legacy
 * sessions on disk, sessions written by buggy earlier shell builds).
 * Unknown values fall through to a neutral "unknown" chip rather
 * than disappearing silently from the row.
 */
function OutcomePill({ outcome }: OutcomePillProps): JSX.Element {
  const value = typeof outcome === 'string' && outcome.length > 0 ? outcome : 'unknown'
  const cssKey = KNOWN_OUTCOMES.has(value) ? value : 'unknown'
  return (
    <span
      className={`agent-session__history-outcome agent-session__history-outcome--${cssKey}`}
      title={value}
    >
      {value}
    </span>
  )
}

function previewJson(value: unknown): string {
  if (value === null || value === undefined) return 'null'
  try {
    const s = JSON.stringify(value, null, 2)
    return s.length > 600 ? `${s.slice(0, 600)}…` : s
  } catch {
    return String(value)
  }
}

function formatTimestamp(iso: string): string {
  if (!iso) return ''
  // ISO trims to the local short form for the history list; full
  // timestamp stays inspectable on hover via title attr in CSS.
  try {
    const d = new Date(iso)
    if (Number.isNaN(d.getTime())) return iso
    return d.toLocaleString()
  } catch {
    return iso
  }
}
