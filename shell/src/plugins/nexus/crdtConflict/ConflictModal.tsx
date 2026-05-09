// BL-074 — CRDT conflict resolver modal.
//
// Replaces the BL-074 v0 toast with an interactive surface. Each row
// shows the diverged content (when available) plus three actions:
//
//   • Keep local   — current state is kept, row marked resolved.
//   • Use remote   — dispatch an editor `UpdateBlockContent` op that
//                    overwrites the block with the remote payload's
//                    content. The CRDT publisher records this as a
//                    fresh local op so the user's history reflects
//                    their choice.
//   • Open file    — emit `files:open` so the user can resolve
//                    manually in the editor surface.
//
// "Use remote" is only wired for `concurrent_block_edit` in v1.
// `structural_delete_edit` is shown read-only (Open file only) since
// undoing a delete or re-deleting a re-created block requires more
// thought — punted to a follow-up.

import { useMemo, useState } from 'react'

import type { PluginAPI } from '../../../types/plugin'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'
import { clientLogger } from '../../../clientLogger'
import { applyUseRemote } from './applyResolution'
import { useConflictStore, type ConflictRow } from './conflictStore'
import type { ConflictDetail } from './types'

interface ModalProps {
  /** Plugin API supplied via the registered overlay component
   *  bootstrap. The modal needs `kernel.invoke` to dispatch
   *  `apply_transaction`, `events.emit` to fire `files:open`, and
   *  `notifications.show` for error feedback. */
  api: PluginAPI
}

/** Shorten a UUID-shaped block id for display in the modal header. */
function shortenBlockId(id: string): string {
  if (id.length <= 12) return id
  return `${id.slice(0, 8)}…${id.slice(-4)}`
}

function ResolutionBadge({ row }: { row: ConflictRow }) {
  if (row.resolution === 'pending' && !row.error) return null
  const styles: Record<string, { color: string; label: string }> = {
    kept_local: { color: 'var(--text-success, #4caf50)', label: 'Kept local' },
    used_remote: { color: 'var(--text-success, #4caf50)', label: 'Remote applied' },
    skipped: { color: 'var(--text-muted)', label: 'Skipped' },
    pending: { color: 'var(--text-error, #f44336)', label: '' },
  }
  const s = styles[row.resolution]
  return (
    <span style={{ color: s.color, fontSize: '0.85em', fontStyle: 'italic' }}>
      {row.error ? `Error: ${row.error}` : s.label}
    </span>
  )
}

function ContentBlock({ label, body }: { label: string; body: string | undefined }) {
  return (
    <div style={{ flex: '1 1 0', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 4 }}>
      <span style={{ color: 'var(--text-muted)', fontSize: '0.85em' }}>{label}</span>
      <pre
        style={{
          margin: 0,
          padding: '6px 8px',
          background: 'var(--background-secondary)',
          color: 'var(--text-normal)',
          fontFamily: 'var(--font-monospace)',
          fontSize: '0.85em',
          borderRadius: 'var(--radius-s)',
          maxHeight: 120,
          overflow: 'auto',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
        }}
      >
        {body ?? <span style={{ color: 'var(--text-faint)' }}>(no content snapshot)</span>}
      </pre>
    </div>
  )
}

function StructuralDescription({
  detail,
}: {
  detail: Extract<ConflictDetail, { kind: 'structural_delete_edit' }>
}) {
  const deleteSide = detail.delete_origin === 'local' ? 'You' : 'A peer'
  const editSide = detail.delete_origin === 'local' ? 'a peer' : 'you'
  return (
    <div style={{ color: 'var(--text-normal)', fontSize: '0.9em' }}>
      <p style={{ margin: '0 0 6px 0' }}>
        <strong>{deleteSide}</strong> deleted this block while <strong>{editSide}</strong> edited
        it.
      </p>
      <ContentBlock label="Surviving edit content" body={detail.local_content} />
      <p
        style={{
          margin: '8px 0 0 0',
          color: 'var(--text-muted)',
          fontSize: '0.85em',
          fontStyle: 'italic',
        }}
      >
        Open the file to keep the edit (cancel the delete) or accept the delete (drop the edit).
      </p>
    </div>
  )
}

function ConflictRowView({
  row,
  rowIdx,
  api,
  relpath,
}: {
  row: ConflictRow
  rowIdx: number
  api: PluginAPI
  relpath: string
}) {
  const setResolution = useConflictStore((s) => s.setResolution)
  const [busy, setBusy] = useState(false)
  const { detail } = row

  const onKeepLocal = () => {
    setResolution(rowIdx, 'kept_local')
  }

  const onUseRemote = async () => {
    if (busy) return
    setBusy(true)
    const err = await applyUseRemote(api, relpath, detail)
    if (err) {
      clientLogger.warn('[nexus.crdtConflict] apply_transaction failed:', err)
      setResolution(rowIdx, 'pending', err)
    } else {
      setResolution(rowIdx, 'used_remote')
    }
    setBusy(false)
  }

  const onOpenFile = () => {
    api.events.emit('files:open', { relpath, name: relpath })
    setResolution(rowIdx, 'skipped')
  }

  const canUseRemote =
    detail.kind === 'concurrent_block_edit' &&
    typeof detail.local_content === 'string' &&
    typeof detail.remote_content === 'string'

  return (
    <div
      style={{
        padding: 12,
        borderBottom: '1px solid var(--background-modifier-border)',
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
      }}
    >
      <div style={{ display: 'flex', gap: 8, alignItems: 'baseline', justifyContent: 'space-between' }}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'baseline' }}>
          <span style={{ color: 'var(--text-normal)', fontWeight: 500 }}>
            {detail.kind === 'concurrent_block_edit'
              ? 'Concurrent block edit'
              : 'Delete vs edit'}
          </span>
          <span
            style={{
              color: 'var(--text-muted)',
              fontFamily: 'var(--font-monospace)',
              fontSize: '0.8em',
            }}
            title={detail.block_id}
          >
            block {shortenBlockId(detail.block_id)}
          </span>
        </div>
        <ResolutionBadge row={row} />
      </div>

      {detail.kind === 'concurrent_block_edit' ? (
        <div style={{ display: 'flex', gap: 12, alignItems: 'stretch' }}>
          <ContentBlock label="Local" body={detail.local_content} />
          <ContentBlock label="Remote" body={detail.remote_content} />
        </div>
      ) : (
        <StructuralDescription detail={detail} />
      )}

      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button
          type="button"
          onClick={onKeepLocal}
          disabled={row.resolution !== 'pending' || busy}
          style={btnStyle()}
        >
          Keep local
        </button>
        <button
          type="button"
          onClick={onUseRemote}
          disabled={!canUseRemote || row.resolution !== 'pending' || busy}
          title={canUseRemote ? '' : 'Use the editor to resolve this conflict manually.'}
          style={btnStyle('primary')}
        >
          {busy ? 'Applying…' : 'Use remote'}
        </button>
        <button type="button" onClick={onOpenFile} disabled={busy} style={btnStyle()}>
          Open file
        </button>
      </div>
    </div>
  )
}

function btnStyle(variant: 'default' | 'primary' = 'default'): React.CSSProperties {
  const base: React.CSSProperties = {
    padding: '4px 10px',
    border: '1px solid var(--background-modifier-border)',
    borderRadius: 'var(--radius-s)',
    background: 'var(--background-secondary)',
    color: 'var(--text-normal)',
    cursor: 'pointer',
    fontSize: '0.9em',
  }
  if (variant === 'primary') {
    return {
      ...base,
      background: 'var(--interactive-accent, #3b82f6)',
      color: 'var(--text-on-accent, #fff)',
      borderColor: 'transparent',
    }
  }
  return base
}

export function ConflictModal({ api }: ModalProps) {
  const current = useConflictStore((s) => s.current)
  const dismiss = useConflictStore((s) => s.dismissCurrent)

  const summary = useMemo(() => {
    if (!current) return ''
    const counts = { concurrent_block_edit: 0, structural_delete_edit: 0 }
    for (const r of current.rows) {
      counts[r.detail.kind] += 1
    }
    const parts: string[] = []
    if (counts.concurrent_block_edit > 0) {
      parts.push(
        `${counts.concurrent_block_edit} concurrent block edit${counts.concurrent_block_edit === 1 ? '' : 's'}`,
      )
    }
    if (counts.structural_delete_edit > 0) {
      parts.push(
        `${counts.structural_delete_edit} delete-vs-edit conflict${counts.structural_delete_edit === 1 ? '' : 's'}`,
      )
    }
    return parts.join(', ')
  }, [current])

  if (!current) return null

  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      dismiss()
    }
  }

  return (
    <Modal>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="nexus-crdt-conflict-title"
        onKeyDown={onKey}
        onClick={(e) => {
          if (e.target === e.currentTarget) dismiss()
        }}
        style={{
          position: 'fixed',
          inset: 0,
          background: 'rgba(0, 0, 0, 0.55)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          zIndex: zIndex.overlayModal,
          pointerEvents: 'auto',
          padding: 32,
        }}
      >
        <div
          style={{
            width: 'min(720px, 100%)',
            maxHeight: '85vh',
            background: 'var(--background-primary)',
            color: 'var(--text-normal)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            boxShadow: '0 12px 48px rgba(0, 0, 0, 0.4)',
            fontFamily: 'var(--font-interface)',
            fontSize: 'var(--ui-size, 13px)',
            display: 'flex',
            flexDirection: 'column',
            overflow: 'hidden',
          }}
        >
          <div
            id="nexus-crdt-conflict-title"
            style={{
              padding: '12px 14px',
              borderBottom: '1px solid var(--background-modifier-border)',
              display: 'flex',
              flexDirection: 'column',
              gap: 2,
            }}
          >
            <span style={{ color: 'var(--text-normal)', fontWeight: 500 }}>
              Merge needs review
            </span>
            <span style={{ color: 'var(--text-muted)', fontSize: '0.85em' }}>
              {current.relpath} — {summary}
            </span>
          </div>
          <div style={{ flex: '1 1 auto', overflowY: 'auto' }}>
            {current.rows.map((row, i) => (
              <ConflictRowView
                key={`${current.id}-${i}`}
                row={row}
                rowIdx={i}
                api={api}
                relpath={current.relpath}
              />
            ))}
          </div>
          <div
            style={{
              padding: '8px 14px',
              borderTop: '1px solid var(--background-modifier-border)',
              display: 'flex',
              justifyContent: 'flex-end',
            }}
          >
            <button type="button" onClick={dismiss} style={btnStyle('primary')}>
              Done
            </button>
          </div>
        </div>
      </div>
    </Modal>
  )
}
