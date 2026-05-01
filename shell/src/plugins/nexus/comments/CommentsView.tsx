import { useState } from 'react'
import { useCommentsStore } from './commentsStore'
import type { CommentsApi } from './commentsApi'
import type { Comment, Thread } from './types'

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/** Short-ish display of a uuid (`xxxxxxxx…`). */
function shortId(id: string): string {
  return id.slice(0, 8)
}

/** RFC3339 → "2026-04-30 10:31" (UTC, defensively). */
function fmtTimestamp(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const pad = (n: number) => n.toString().padStart(2, '0')
  return (
    `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ` +
    `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`
  )
}

interface ViewProps {
  /** Lazily-bound comments API. The plugin sets this once `kernel`
   *  is available — until then the View renders an empty state. */
  api: CommentsApi | null
  /** Best-effort author name to attribute comments to. May be empty. */
  author: string | null
}

/**
 * Right-panel inspector listing every comment thread anchored to a
 * block in the active editor tab. Threads are rendered oldest-first;
 * each thread renders its comments in sequence with reply / edit /
 * delete controls. The thread header carries a resolve toggle and a
 * delete-thread affordance.
 *
 * BL-050 Phase 2: this pane provides the user-visible surface for the
 * existing storage backend. It does NOT yet create new threads — that
 * UX belongs to the editor margin gutter (Phase 3). The `createThread`
 * handler is exercised through the `commentsApi` module so external
 * callers (e.g. a future right-click menu) have a typed entry point.
 */
export function CommentsView({ api, author }: ViewProps) {
  const currentRelpath = useCommentsStore((s) => s.currentRelpath)
  const threads = useCommentsStore((s) => s.threads)
  const loading = useCommentsStore((s) => s.loading)
  const error = useCommentsStore((s) => s.error)

  const header = currentRelpath ? (
    <div
      style={{
        padding: '8px 14px',
        borderBottom: '1px solid var(--divider-color)',
        fontSize: 11,
        fontFamily: 'var(--font-interface)',
        color: 'var(--text-faint)',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
      title={currentRelpath}
    >
      Comments on{' '}
      <span style={{ color: 'var(--text-normal)' }}>{basename(currentRelpath)}</span>
    </div>
  ) : null

  let body: React.ReactNode
  if (!currentRelpath) {
    body = (
      <StateMessage color="var(--text-faint)">
        Open a file to see its comment threads.
      </StateMessage>
    )
  } else if (error) {
    body = <StateMessage color="var(--risk)">{error}</StateMessage>
  } else if (loading) {
    body = <StateMessage color="var(--text-muted)">Loading…</StateMessage>
  } else if (threads.length === 0) {
    body = (
      <StateMessage color="var(--text-faint)">
        No comments yet. Use the editor margin to comment on a block.
      </StateMessage>
    )
  } else if (!api) {
    body = (
      <StateMessage color="var(--text-muted)">
        Kernel not ready. Threads will load shortly.
      </StateMessage>
    )
  } else {
    body = (
      <div style={{ overflowY: 'auto', flex: 1 }}>
        {threads.map((t) => (
          <ThreadRow
            key={t.id}
            thread={t}
            api={api}
            relpath={currentRelpath}
            author={author}
          />
        ))}
      </div>
    )
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        width: '100%',
      }}
    >
      {header}
      {body}
    </div>
  )
}

function StateMessage({
  children,
  color,
}: {
  children: React.ReactNode
  color: string
}) {
  return (
    <div
      style={{
        padding: '12px 14px',
        color,
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
      }}
    >
      {children}
    </div>
  )
}

interface ThreadRowProps {
  thread: Thread
  api: CommentsApi
  relpath: string
  author: string | null
}

function ThreadRow({ thread, api, relpath, author }: ThreadRowProps) {
  const [reply, setReply] = useState('')
  const [busy, setBusy] = useState(false)
  const [rowError, setRowError] = useState<string | null>(null)

  const upsert = useCommentsStore((s) => s.upsertThread)
  const remove = useCommentsStore((s) => s.removeThread)

  const submitReply = async () => {
    const body = reply.trim()
    if (!body || busy) return
    setBusy(true)
    setRowError(null)
    try {
      const created = await api.addReply({
        filePath: relpath,
        threadId: thread.id,
        body,
        author: author ?? undefined,
      })
      // Echo the new comment into the existing thread; avoids a full
      // re-list round-trip and keeps the UI responsive.
      upsert({
        ...thread,
        comments: [...thread.comments, created],
      })
      setReply('')
    } catch (err) {
      setRowError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const toggleResolved = async () => {
    if (busy) return
    setBusy(true)
    setRowError(null)
    try {
      const updated = await api.setResolved({
        filePath: relpath,
        threadId: thread.id,
        resolved: !thread.resolved,
        author: author ?? undefined,
      })
      upsert(updated)
    } catch (err) {
      setRowError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  const deleteThread = async () => {
    if (busy) return
    setBusy(true)
    setRowError(null)
    try {
      await api.deleteThread({ filePath: relpath, threadId: thread.id })
      remove(thread.id)
    } catch (err) {
      setRowError(err instanceof Error ? err.message : String(err))
      setBusy(false)
    }
  }

  return (
    <div
      style={{
        padding: '10px 14px',
        borderBottom: '1px solid var(--divider-color)',
        opacity: thread.resolved ? 0.65 : 1,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 6,
          fontSize: 11,
          fontFamily: 'var(--font-monospace)',
          color: 'var(--text-faint)',
        }}
      >
        <span title={`Block ${thread.block_id}`}>
          ^{shortId(thread.block_id)}
        </span>
        <div style={{ display: 'flex', gap: 8 }}>
          <button
            type="button"
            onClick={toggleResolved}
            disabled={busy}
            style={miniButtonStyle}
          >
            {thread.resolved ? 'Reopen' : 'Resolve'}
          </button>
          <button
            type="button"
            onClick={deleteThread}
            disabled={busy}
            style={miniButtonStyle}
          >
            Delete thread
          </button>
        </div>
      </div>

      {thread.comments.map((c) => (
        <CommentRow
          key={c.id}
          comment={c}
          thread={thread}
          api={api}
          relpath={relpath}
          canDelete={thread.comments.length > 1}
        />
      ))}

      {!thread.resolved && (
        <div style={{ marginTop: 6 }}>
          <textarea
            value={reply}
            onChange={(e) => setReply(e.target.value)}
            placeholder="Reply…"
            rows={2}
            disabled={busy}
            style={textareaStyle}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                e.preventDefault()
                void submitReply()
              }
            }}
          />
          <div
            style={{
              display: 'flex',
              justifyContent: 'flex-end',
              marginTop: 4,
            }}
          >
            <button
              type="button"
              onClick={() => void submitReply()}
              disabled={busy || reply.trim().length === 0}
              style={miniButtonStyle}
            >
              Reply
            </button>
          </div>
        </div>
      )}

      {rowError && (
        <div
          style={{
            color: 'var(--risk)',
            fontSize: 11,
            marginTop: 4,
            fontFamily: 'var(--font-interface)',
          }}
        >
          {rowError}
        </div>
      )}
    </div>
  )
}

interface CommentRowProps {
  comment: Comment
  thread: Thread
  api: CommentsApi
  relpath: string
  /** False when this is the only comment in its thread — deleting the
   *  last comment isn't allowed by the storage backend. */
  canDelete: boolean
}

function CommentRow({
  comment,
  thread,
  api,
  relpath,
  canDelete,
}: CommentRowProps) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(comment.body)
  const [busy, setBusy] = useState(false)
  const upsert = useCommentsStore((s) => s.upsertThread)

  const saveEdit = async () => {
    const body = draft.trim()
    if (!body || body === comment.body || busy) {
      setEditing(false)
      setDraft(comment.body)
      return
    }
    setBusy(true)
    try {
      const updated = await api.editComment({
        filePath: relpath,
        threadId: thread.id,
        commentId: comment.id,
        body,
      })
      upsert({
        ...thread,
        comments: thread.comments.map((c) =>
          c.id === comment.id ? updated : c,
        ),
      })
      setEditing(false)
    } catch {
      // Bail without echoing — the thread will be re-listed on next
      // tab change. Keep the editor open so the draft isn't lost.
      setBusy(false)
      return
    }
    setBusy(false)
  }

  const deleteSelf = async () => {
    if (!canDelete || busy) return
    setBusy(true)
    try {
      await api.deleteComment({
        filePath: relpath,
        threadId: thread.id,
        commentId: comment.id,
      })
      upsert({
        ...thread,
        comments: thread.comments.filter((c) => c.id !== comment.id),
      })
    } catch {
      setBusy(false)
    }
  }

  return (
    <div
      style={{
        padding: '6px 0',
        borderTop: '1px dashed var(--divider-color)',
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          color: 'var(--text-faint)',
          fontSize: 11,
        }}
      >
        <span>
          <span style={{ color: 'var(--text-normal)' }}>
            {comment.author ?? 'anonymous'}
          </span>
          {' · '}
          {fmtTimestamp(comment.created_at)}
          {comment.updated_at && ' · edited'}
        </span>
      </div>

      {editing ? (
        <>
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            rows={2}
            disabled={busy}
            style={textareaStyle}
            autoFocus
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                e.preventDefault()
                void saveEdit()
              }
              if (e.key === 'Escape') {
                e.preventDefault()
                setEditing(false)
                setDraft(comment.body)
              }
            }}
          />
          <div style={{ display: 'flex', gap: 6, justifyContent: 'flex-end' }}>
            <button
              type="button"
              onClick={() => {
                setEditing(false)
                setDraft(comment.body)
              }}
              style={miniButtonStyle}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => void saveEdit()}
              disabled={busy}
              style={miniButtonStyle}
            >
              Save
            </button>
          </div>
        </>
      ) : (
        <>
          <div
            style={{
              color: 'var(--text-normal)',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              marginTop: 2,
            }}
          >
            {comment.body}
          </div>
          {comment.mentions.length > 0 && (
            <div
              style={{
                color: 'var(--text-muted)',
                fontSize: 11,
                marginTop: 2,
              }}
            >
              {comment.mentions.map((m) => `@${m}`).join(' ')}
            </div>
          )}
          <div
            style={{
              display: 'flex',
              gap: 6,
              justifyContent: 'flex-end',
              marginTop: 2,
            }}
          >
            <button
              type="button"
              onClick={() => setEditing(true)}
              style={miniButtonStyle}
            >
              Edit
            </button>
            <button
              type="button"
              onClick={() => void deleteSelf()}
              disabled={!canDelete || busy}
              title={
                canDelete
                  ? undefined
                  : 'Cannot delete the only remaining comment in a thread.'
              }
              style={miniButtonStyle}
            >
              Delete
            </button>
          </div>
        </>
      )}
    </div>
  )
}

const miniButtonStyle: React.CSSProperties = {
  background: 'transparent',
  border: '1px solid var(--divider-color)',
  color: 'var(--text-faint)',
  fontSize: 11,
  padding: '2px 6px',
  borderRadius: 3,
  cursor: 'pointer',
  fontFamily: 'var(--font-interface)',
}

const textareaStyle: React.CSSProperties = {
  width: '100%',
  background: 'var(--background-primary)',
  color: 'var(--text-normal)',
  border: '1px solid var(--divider-color)',
  borderRadius: 3,
  fontFamily: 'var(--font-interface)',
  fontSize: 12,
  padding: '4px 6px',
  resize: 'vertical',
  boxSizing: 'border-box',
}
