import { useCallback, useEffect, useMemo } from 'react'

import { useGitPanelStore } from '../gitPanelStore'
import { getGitPanelApi } from '../gitPanelRuntime'
import {
  applyAll,
  applyResolution,
  parseConflicts,
  type ConflictHunk,
} from './conflictParser'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'

interface ReadResp {
  bytes: number[]
}

function decodeUtf8(bytes: number[]): string {
  try {
    return new TextDecoder('utf-8', { fatal: false }).decode(new Uint8Array(bytes))
  } catch {
    return ''
  }
}

function encodeUtf8(text: string): number[] {
  return Array.from(new TextEncoder().encode(text))
}

/**
 * BL-084 conflict-resolution panel for a single file.
 *
 * Renders the working-tree contents with `<<<<<<<` / `=======` /
 * `>>>>>>>` markers parsed into hunks, surfacing per-hunk
 * "Use ours" / "Use theirs" buttons plus whole-file accept-all
 * shortcuts. Resolved content is written back via
 * `com.nexus.storage::write_file`; staging + commit go through the
 * existing Changes-tab UI.
 *
 * Three-way visualisation against `conflict_versions` (base / ours /
 * theirs side-by-side) is deferred — the conflict-marker form already
 * shows ours and theirs inline, and `applyResolution` operates on the
 * working tree, not the index versions.
 */
export function ConflictView({ relpath }: { relpath: string }) {
  const conflict = useGitPanelStore((s) => s.conflict)
  const setConflict = useGitPanelStore((s) => s.setConflict)
  const resetConflict = useGitPanelStore((s) => s.resetConflict)

  const loadContent = useCallback(async () => {
    setConflict({ error: null })
    try {
      const api = getGitPanelApi()
      const resp = await api.kernel.invoke<ReadResp>(STORAGE_PLUGIN_ID, 'read_file', {
        path: relpath,
      })
      setConflict({ content: decodeUtf8(resp.bytes ?? []) })
    } catch (err) {
      setConflict({
        content: null,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  }, [relpath, setConflict])

  useEffect(() => {
    resetConflict()
    void loadContent()
    return () => {
      resetConflict()
    }
  }, [relpath, loadContent, resetConflict])

  const parsed = useMemo(
    () => (conflict.content !== null ? parseConflicts(conflict.content) : { hunks: [] }),
    [conflict.content],
  )

  const writeBack = useCallback(
    async (next: string) => {
      setConflict({ saving: true, error: null })
      try {
        const api = getGitPanelApi()
        await api.kernel.invoke(STORAGE_PLUGIN_ID, 'write_file', {
          path: relpath,
          bytes: encodeUtf8(next),
        })
        setConflict({ content: next, saving: false })
      } catch (err) {
        setConflict({
          saving: false,
          error: err instanceof Error ? err.message : String(err),
        })
      }
    },
    [relpath, setConflict],
  )

  const onPickHunk = useCallback(
    (hunk: ConflictHunk, side: 'ours' | 'theirs') => {
      if (conflict.content === null) return
      const next = applyResolution(conflict.content, hunk, side === 'ours' ? hunk.ours : hunk.theirs)
      void writeBack(next)
    },
    [conflict.content, writeBack],
  )

  const onAcceptAll = useCallback(
    (side: 'ours' | 'theirs') => {
      if (conflict.content === null) return
      const next = applyAll(conflict.content, parsed, side)
      void writeBack(next)
    },
    [conflict.content, parsed, writeBack],
  )

  if (conflict.error) {
    return (
      <div style={{ padding: 12, fontFamily: 'var(--font-interface)', fontSize: 12, color: 'var(--text-error, #c53030)' }}>
        Failed to read {relpath}: {conflict.error}
      </div>
    )
  }
  if (conflict.content === null) {
    return (
      <div style={{ padding: 12, fontFamily: 'var(--font-interface)', fontSize: 12, color: 'var(--text-muted)' }}>
        Loading {relpath}…
      </div>
    )
  }

  // No remaining markers: file is resolved on disk; staging + commit
  // happen through the regular Changes-tab UI. Surface a hint so the
  // user knows what to do next.
  if (parsed.hunks.length === 0) {
    return (
      <div style={{ padding: 12, fontFamily: 'var(--font-interface)', fontSize: 12 }}>
        <div style={{ color: 'var(--text-success, #38a169)', fontWeight: 600, marginBottom: 6 }}>
          No conflict markers remain in {relpath}.
        </div>
        <div style={{ color: 'var(--text-muted)' }}>
          Stage the file from the Changes tab to mark this conflict as resolved, then commit when all
          conflicts are handled.
        </div>
      </div>
    )
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      {/* Toolbar: whole-file accept-all shortcuts. */}
      <div
        style={{
          display: 'flex',
          gap: 6,
          padding: '6px 8px',
          borderBottom: '1px solid var(--background-modifier-border)',
          fontFamily: 'var(--font-interface)',
          fontSize: 12,
          alignItems: 'center',
          flexShrink: 0,
        }}
      >
        <span style={{ color: 'var(--text-muted)', marginRight: 'auto' }}>
          {parsed.hunks.length} conflict{parsed.hunks.length === 1 ? '' : 's'}
        </span>
        <button
          type="button"
          onClick={() => onAcceptAll('ours')}
          disabled={conflict.saving}
          style={SECONDARY_BUTTON}
        >
          Accept all ours
        </button>
        <button
          type="button"
          onClick={() => onAcceptAll('theirs')}
          disabled={conflict.saving}
          style={SECONDARY_BUTTON}
        >
          Accept all theirs
        </button>
      </div>

      {/* Hunks. */}
      <div style={{ flex: 1, overflowY: 'auto', padding: 8 }}>
        {parsed.hunks.map((hunk, idx) => (
          <HunkBlock
            key={`${hunk.start}-${hunk.end}`}
            index={idx + 1}
            hunk={hunk}
            disabled={conflict.saving}
            onPick={(side) => onPickHunk(hunk, side)}
          />
        ))}
      </div>
    </div>
  )
}

function HunkBlock({
  index,
  hunk,
  disabled,
  onPick,
}: {
  index: number
  hunk: ConflictHunk
  disabled: boolean
  onPick: (side: 'ours' | 'theirs') => void
}) {
  return (
    <div
      style={{
        marginBottom: 10,
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 4,
        background: 'var(--background-primary)',
        fontFamily: 'var(--font-monospace)',
        fontSize: 12,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '4px 8px',
          background: 'var(--background-secondary)',
          borderBottom: '1px solid var(--background-modifier-border)',
          fontFamily: 'var(--font-interface)',
        }}
      >
        <span style={{ color: 'var(--text-muted)' }}>Hunk {index}</span>
      </div>

      <SidePanel
        label={`Ours${hunk.oursLabel ? ` (${hunk.oursLabel})` : ''}`}
        body={hunk.ours}
        accent="var(--diff-add-bg, rgba(46,160,67,0.15))"
        onAccept={() => onPick('ours')}
        disabled={disabled}
      />
      {hunk.base !== null && (
        <SidePanel
          label="Base"
          body={hunk.base}
          accent="var(--background-secondary)"
          onAccept={null}
          disabled={disabled}
        />
      )}
      <SidePanel
        label={`Theirs${hunk.theirsLabel ? ` (${hunk.theirsLabel})` : ''}`}
        body={hunk.theirs}
        accent="var(--diff-rem-bg, rgba(229,62,62,0.12))"
        onAccept={() => onPick('theirs')}
        disabled={disabled}
      />
    </div>
  )
}

function SidePanel({
  label,
  body,
  accent,
  onAccept,
  disabled,
}: {
  label: string
  body: string
  accent: string
  onAccept: (() => void) | null
  disabled: boolean
}) {
  return (
    <div style={{ borderTop: '1px solid var(--background-modifier-border)', background: accent }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '4px 8px',
          fontFamily: 'var(--font-interface)',
          fontSize: 11,
          color: 'var(--text-muted)',
        }}
      >
        <span style={{ marginRight: 'auto' }}>{label}</span>
        {onAccept && (
          <button type="button" onClick={onAccept} disabled={disabled} style={SECONDARY_BUTTON}>
            Use this side
          </button>
        )}
      </div>
      <pre
        style={{
          margin: 0,
          padding: '6px 8px',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
          fontFamily: 'inherit',
        }}
      >
        {body || ' '}
      </pre>
    </div>
  )
}

const SECONDARY_BUTTON: React.CSSProperties = {
  padding: '3px 8px',
  fontSize: 11,
  fontFamily: 'var(--font-interface)',
  background: 'var(--interactive-normal)',
  color: 'var(--text-normal)',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 3,
  cursor: 'pointer',
}
