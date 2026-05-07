// shell/src/plugins/nexus/editor/DiffView.tsx
//
// BL-079 — modal-style diff viewer.
//
// Renders the `com.nexus.git::diff_file` response as a unified
// list of hunks, each with three column types:
//   - Removed lines, red background
//   - Added lines, green background
//   - Context lines, neutral
// Each hunk gets a header row showing the line ranges. Stage and
// Revert are out-of-scope here (the gutter / git panel own those
// flows); this view is read-only and meant for "what's changed in
// this file?" inspection.

import { useEffect, useState } from 'react'
import type { KernelAPI } from '../../../types/plugin'

const PLUGIN_ID = 'com.nexus.git'
const CMD_DIFF_FILE = 'diff_file'

interface GitDiffLine {
  kind: 'Context' | 'Added' | 'Removed'
  content: string
}

interface GitDiffHunk {
  old_start: number
  old_count: number
  new_start: number
  new_count: number
  lines: GitDiffLine[]
}

interface DiffViewProps {
  kernel: KernelAPI
  relpath: string
  onClose: () => void
}

export function DiffView({ kernel, relpath, onClose }: DiffViewProps) {
  const [hunks, setHunks] = useState<GitDiffHunk[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        if (!(await kernel.available())) {
          if (!cancelled) setError('Kernel not available')
          return
        }
        const resp = await kernel.invoke<GitDiffHunk[]>(
          PLUGIN_ID,
          CMD_DIFF_FILE,
          { path: relpath },
        )
        if (cancelled) return
        setHunks(resp ?? [])
      } catch (err) {
        if (cancelled) return
        setError(err instanceof Error ? err.message : String(err))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [kernel, relpath])

  return (
    <div className="nexus-diff-view-overlay" onClick={onClose}>
      <div
        className="nexus-diff-view-modal"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label={`Diff for ${relpath}`}
      >
        <header className="nexus-diff-view-header">
          <h3>
            Changes · <code>{relpath}</code>
          </h3>
          <button
            type="button"
            className="nexus-diff-view-close"
            onClick={onClose}
            aria-label="Close"
          >
            ✕
          </button>
        </header>
        <div className="nexus-diff-view-body">
          {error && (
            <div className="nexus-diff-view-error" role="alert">
              {error}
            </div>
          )}
          {hunks === null && !error && (
            <div className="nexus-diff-view-loading">Loading diff…</div>
          )}
          {hunks !== null && hunks.length === 0 && !error && (
            <div className="nexus-diff-view-empty">
              No working-copy changes for this file.
            </div>
          )}
          {hunks !== null &&
            hunks.map((hunk, idx) => (
              <DiffHunk key={idx} hunk={hunk} />
            ))}
        </div>
      </div>
    </div>
  )
}

function DiffHunk({ hunk }: { hunk: GitDiffHunk }) {
  return (
    <section className="nexus-diff-view-hunk">
      <header className="nexus-diff-view-hunk-header">
        @@ -{hunk.old_start},{hunk.old_count} +{hunk.new_start},
        {hunk.new_count} @@
      </header>
      <pre className="nexus-diff-view-hunk-body">
        {hunk.lines.map((line, idx) => (
          <DiffLine key={idx} line={line} />
        ))}
      </pre>
    </section>
  )
}

function DiffLine({ line }: { line: GitDiffLine }) {
  const cls =
    line.kind === 'Added'
      ? 'nexus-diff-view-line-added'
      : line.kind === 'Removed'
        ? 'nexus-diff-view-line-removed'
        : 'nexus-diff-view-line-context'
  const sigil = line.kind === 'Added' ? '+' : line.kind === 'Removed' ? '-' : ' '
  return (
    <span className={`nexus-diff-view-line ${cls}`}>
      <span className="nexus-diff-view-sigil">{sigil}</span>
      {line.content}
      {'\n'}
    </span>
  )
}
