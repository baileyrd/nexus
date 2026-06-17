/**
 * Pane-mode view for `nexus.sessions` (RFC 0008, Phase 5.4).
 *
 * Renders the session forest from `session_list` (parent/child via
 * `parent_id` / `branch_point`) and drives the fork verbs — resume / branch /
 * rewind — plus named checkpoints. A thin reader of [`useSessionsStore`]; every
 * mutation flows through callbacks the runtime in `index.ts` supplies, which is
 * what lets the runtime be unit-tested in isolation.
 */

import { useMemo, useState } from 'react'

import './sessions.css'
import { buildForest, flattenForest } from './sessionTree'
import { useSessionsStore } from './sessionsStore'

export interface SessionTreeViewProps {
  onRefresh: () => void
  onSelect: (id: string) => void
  onResume: (id: string, message: string) => void
  onBranch: (id: string, round: number, message: string) => void
  onRewind: (id: string, round: number, message: string) => void
  onCheckpoint: (id: string, round: number, name: string) => void
  onDeleteCheckpoint: (name: string) => void
  onDeleteSession: (id: string) => void
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s
  return `${s.slice(0, max)}…`
}

export function SessionTreeView(props: SessionTreeViewProps) {
  const {
    nodes,
    loading,
    error,
    selectedId,
    transcript,
    transcriptError,
    checkpoints,
    busy,
  } = useSessionsStore()

  const [message, setMessage] = useState('')
  const [cpName, setCpName] = useState('')
  const [cpRound, setCpRound] = useState(1)

  const rows = useMemo(() => flattenForest(buildForest(nodes)), [nodes])
  const messageEmpty = message.trim().length === 0

  return (
    <div className="nx-sessions">
      <aside className="nx-sessions__tree">
        <header className="nx-sessions__tree-head">
          <span className="nx-sessions__title">Sessions</span>
          <button
            type="button"
            className="nx-sessions__btn"
            onClick={props.onRefresh}
            disabled={loading}
          >
            {loading ? '…' : 'Refresh'}
          </button>
        </header>
        {error ? <p className="nx-sessions__error">{error}</p> : null}
        {!error && rows.length === 0 && !loading ? (
          <p className="nx-sessions__empty">No sessions yet.</p>
        ) : null}
        <ul className="nx-sessions__list">
          {rows.map((node) => (
            <li key={node.id}>
              <button
                type="button"
                className={
                  'nx-sessions__row' +
                  (node.id === selectedId ? ' nx-sessions__row--active' : '')
                }
                style={{ paddingLeft: `${8 + node.depth * 16}px` }}
                onClick={() => props.onSelect(node.id)}
                title={node.goal}
              >
                <span className="nx-sessions__marker">
                  {node.parentId ? '↳' : '•'}
                </span>
                <span className="nx-sessions__goal">
                  {truncate(node.goal || '(no goal)', 48)}
                </span>
                <span className={`nx-sessions__outcome nx-sessions__outcome--${node.outcome}`}>
                  {node.outcome}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <section className="nx-sessions__detail">
        {!selectedId ? (
          <p className="nx-sessions__hint">Select a session to view its transcript.</p>
        ) : transcriptError ? (
          <p className="nx-sessions__error">{transcriptError}</p>
        ) : !transcript ? (
          <p className="nx-sessions__hint">Loading transcript…</p>
        ) : (
          <>
            <header className="nx-sessions__detail-head">
              <div>
                <h3 className="nx-sessions__detail-goal">{transcript.goal}</h3>
                <span className="nx-sessions__detail-meta">
                  {transcript.outcome} · {transcript.rounds.length} round(s)
                </span>
              </div>
              <button
                type="button"
                className="nx-sessions__btn nx-sessions__btn--danger"
                onClick={() => props.onDeleteSession(transcript.id)}
                disabled={busy}
              >
                Delete
              </button>
            </header>

            <div className="nx-sessions__compose">
              <input
                type="text"
                className="nx-sessions__input"
                aria-label="Message for resume, branch, or rewind"
                placeholder="Message for resume / branch / rewind…"
                value={message}
                onChange={(e) => setMessage(e.target.value)}
              />
              <button
                type="button"
                className="nx-sessions__btn"
                onClick={() => props.onResume(transcript.id, message)}
                disabled={busy || messageEmpty}
                title="Continue this session from its tip with the message above"
              >
                Resume
              </button>
            </div>

            <ol className="nx-sessions__rounds">
              {transcript.rounds.map((round) => (
                <li key={round.round} className="nx-sessions__round">
                  <div className="nx-sessions__round-head">
                    <span className="nx-sessions__round-n">round {round.round}</span>
                    <span className="nx-sessions__round-actions">
                      <button
                        type="button"
                        className="nx-sessions__btn nx-sessions__btn--sm"
                        onClick={() => props.onBranch(transcript.id, round.round, message)}
                        disabled={busy || messageEmpty}
                        title="Fork a parallel line from this round with the message above"
                      >
                        Branch
                      </button>
                      <button
                        type="button"
                        className="nx-sessions__btn nx-sessions__btn--sm"
                        onClick={() => props.onRewind(transcript.id, round.round, message)}
                        disabled={busy}
                        title="Non-destructively re-run from this round (message optional)"
                      >
                        Rewind
                      </button>
                    </span>
                  </div>
                  {round.text ? (
                    <p className="nx-sessions__round-text">{truncate(round.text, 240)}</p>
                  ) : null}
                  {round.tool_calls.length > 0 ? (
                    <span className="nx-sessions__round-tools">
                      {round.tool_calls.length} tool call(s)
                    </span>
                  ) : null}
                </li>
              ))}
            </ol>

            <div className="nx-sessions__checkpoint-form">
              <span className="nx-sessions__subtitle">Checkpoint</span>
              <select
                className="nx-sessions__select"
                aria-label="Checkpoint round"
                value={cpRound}
                onChange={(e) => setCpRound(Number(e.target.value))}
              >
                {transcript.rounds.map((r) => (
                  <option key={r.round} value={r.round}>
                    round {r.round}
                  </option>
                ))}
              </select>
              <input
                type="text"
                className="nx-sessions__input"
                aria-label="Checkpoint name"
                placeholder="name…"
                value={cpName}
                onChange={(e) => setCpName(e.target.value)}
              />
              <button
                type="button"
                className="nx-sessions__btn nx-sessions__btn--sm"
                onClick={() => {
                  props.onCheckpoint(transcript.id, cpRound, cpName)
                  setCpName('')
                }}
                disabled={cpName.trim().length === 0}
              >
                Save
              </button>
            </div>
          </>
        )}

        {checkpoints.length > 0 ? (
          <div className="nx-sessions__checkpoints">
            <span className="nx-sessions__subtitle">Checkpoints</span>
            <ul className="nx-sessions__cp-list">
              {checkpoints.map((cp) => (
                <li key={cp.name} className="nx-sessions__cp">
                  <button
                    type="button"
                    className="nx-sessions__cp-link"
                    onClick={() => props.onSelect(cp.sessionId)}
                    title={`${cp.sessionId} @ round ${cp.round}`}
                  >
                    {cp.name}
                  </button>
                  <span className="nx-sessions__cp-meta">@ round {cp.round}</span>
                  <button
                    type="button"
                    className="nx-sessions__cp-rm"
                    onClick={() => props.onDeleteCheckpoint(cp.name)}
                    aria-label={`Remove checkpoint ${cp.name}`}
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </section>
    </div>
  )
}
