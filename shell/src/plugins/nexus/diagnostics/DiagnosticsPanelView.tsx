// BL-141 follow-up — diagnostics panel view.
//
// Renders the URI-keyed diagnostics map grouped by file. Each file is a
// section with the relpath header and one row per diagnostic; clicking
// a row raises the file tab and scrolls to the diagnostic's start
// position. The header carries a single button — "Open all in
// multibuffer" — wired by the plugin's activate hook.

import { useMemo } from 'react'
import {
  bucketCounts,
  composeHeader,
  totalBuckets,
  useDiagnosticsStore,
  type SeverityBuckets,
} from './diagnosticsStore'
import type { LspDiagnostic } from '../editor/cm/lspIpc.ts'
import { severityTag, uriToRelpath } from '../editor/cm/lspToExcerpts.ts'

interface Props {
  forgeRoot: string | null
  onOpenInMultibuffer(): void
  onOpenDiagnostic(uri: string, diag: LspDiagnostic): void
}

interface FileGroup {
  uri: string
  relpath: string
  diagnostics: LspDiagnostic[]
  buckets: SeverityBuckets
}

/** Build the ordered file-group list for the panel. Files outside the
 *  forge are dropped (their diagnostics can't be opened by the
 *  multibuffer / files:open paths anyway). Within a file, diagnostics
 *  are sorted by `(line, character)` so the panel reads top-to-bottom.
 *  Files are sorted by relpath so the order is stable across renders.
 *  Exported for tests. */
export function buildFileGroups(
  byUri: ReadonlyMap<string, LspDiagnostic[]>,
  forgeRoot: string | null,
): FileGroup[] {
  if (!forgeRoot) return []
  const groups: FileGroup[] = []
  for (const [uri, diags] of byUri) {
    const relpath = uriToRelpath(uri, forgeRoot)
    if (relpath === null || relpath === '') continue
    if (!Array.isArray(diags) || diags.length === 0) continue
    const sorted = diags.slice().sort((a, b) => {
      const al = a?.range?.start?.line ?? 0
      const bl = b?.range?.start?.line ?? 0
      if (al !== bl) return al - bl
      const ac = a?.range?.start?.character ?? 0
      const bc = b?.range?.start?.character ?? 0
      return ac - bc
    })
    groups.push({
      uri,
      relpath,
      diagnostics: sorted,
      buckets: bucketCounts(sorted),
    })
  }
  groups.sort((a, b) => a.relpath.localeCompare(b.relpath))
  return groups
}

function severityColor(tag: string): string {
  switch (tag) {
    case 'error':
      return 'var(--text-error, #ef4444)'
    case 'warn':
      return 'var(--text-warning, #f59e0b)'
    case 'info':
      return 'var(--text-info, #3b82f6)'
    case 'hint':
    default:
      return 'var(--text-muted, #888)'
  }
}

function SeverityChip({ tag }: { tag: string }) {
  return (
    <span
      style={{
        display: 'inline-block',
        minWidth: 36,
        textAlign: 'center',
        fontSize: 10,
        textTransform: 'uppercase',
        letterSpacing: 0.4,
        padding: '1px 4px',
        borderRadius: 3,
        color: severityColor(tag),
        border: `1px solid ${severityColor(tag)}`,
      }}
    >
      {tag}
    </span>
  )
}

export function DiagnosticsPanelView(props: Props) {
  const byUri = useDiagnosticsStore((s) => s.byUri)
  const groups = useMemo(
    () => buildFileGroups(byUri, props.forgeRoot),
    [byUri, props.forgeRoot],
  )
  const totals = useMemo(() => totalBuckets(byUri), [byUri])
  const headerSummary = composeHeader(totals)
  const totalCount =
    totals.error + totals.warn + totals.info + totals.hint
  const isEmpty = groups.length === 0

  return (
    <div
      className="nexus-diagnostics-panel"
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        fontSize: 13,
      }}
    >
      <header
        style={{
          padding: '8px 12px',
          borderBottom: '1px solid var(--border, #2a2a2a)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
        }}
      >
        <div>
          <strong style={{ fontSize: 14 }}>Diagnostics</strong>
          <span
            style={{ marginLeft: 8, color: 'var(--text-muted, #888)' }}
          >
            {headerSummary || 'no issues'}
          </span>
        </div>
        <button
          type="button"
          disabled={totalCount === 0}
          onClick={props.onOpenInMultibuffer}
          style={{
            background: 'transparent',
            color:
              totalCount === 0
                ? 'var(--text-muted, #888)'
                : 'var(--text-normal, #ddd)',
            border: '1px solid var(--border, #2a2a2a)',
            borderRadius: 4,
            padding: '2px 8px',
            cursor: totalCount === 0 ? 'not-allowed' : 'pointer',
            fontSize: 12,
          }}
          title="Open every diagnostic in a single editable multibuffer"
        >
          Open all in multibuffer
        </button>
      </header>

      {isEmpty && (
        <div
          style={{
            padding: 16,
            color: 'var(--text-muted, #888)',
          }}
        >
          {props.forgeRoot
            ? 'No diagnostics from any LSP server. Open a code-mode tab to start a server.'
            : 'No workspace open.'}
        </div>
      )}

      {!isEmpty && (
        <div
          style={{
            overflowY: 'auto',
            flex: 1,
          }}
        >
          {groups.map((group) => (
            <section key={group.uri}>
              <header
                style={{
                  padding: '6px 12px',
                  position: 'sticky',
                  top: 0,
                  background: 'var(--surface, #181818)',
                  borderBottom: '1px solid var(--border, #2a2a2a)',
                  display: 'flex',
                  alignItems: 'baseline',
                  gap: 8,
                }}
              >
                <strong>{group.relpath}</strong>
                <span
                  style={{
                    color: 'var(--text-muted, #888)',
                    fontSize: 12,
                  }}
                >
                  {group.diagnostics.length}{' '}
                  {group.diagnostics.length === 1 ? 'issue' : 'issues'}
                </span>
              </header>
              <ul style={{ listStyle: 'none', margin: 0, padding: 0 }}>
                {group.diagnostics.map((d, i) => {
                  const tag = severityTag(d.severity)
                  const startLine = (d.range?.start?.line ?? 0) + 1
                  const message = (d.message ?? '').replace(/\s+/g, ' ').trim()
                  return (
                    <li
                      key={`${i}:${startLine}:${tag}`}
                      onClick={() => props.onOpenDiagnostic(group.uri, d)}
                      style={{
                        padding: '4px 12px 4px 24px',
                        borderBottom: '1px solid var(--border-faint, #222)',
                        display: 'flex',
                        gap: 8,
                        cursor: 'pointer',
                        alignItems: 'baseline',
                      }}
                    >
                      <SeverityChip tag={tag} />
                      <span
                        style={{
                          color: 'var(--text-muted, #888)',
                          fontVariantNumeric: 'tabular-nums',
                          minWidth: 40,
                        }}
                      >
                        L{startLine}
                      </span>
                      <span style={{ flex: 1 }}>{message}</span>
                      {d.source && (
                        <span
                          style={{
                            color: 'var(--text-muted, #888)',
                            fontSize: 11,
                          }}
                        >
                          {d.source}
                        </span>
                      )}
                    </li>
                  )
                })}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  )
}
