// shell/src/plugins/nexus/enrich/EnrichAcceptGate.tsx
//
// BL-045 — accept-gate UI.
//
// A small bottom-right toast-style panel that appears whenever the
// runtime has pushed a fresh `EnrichmentProposal`. Shows the file
// name, the suggested tags / summary / related notes, and two
// buttons: Accept (issues `enrich_apply`) and Dismiss (clears the
// pending slot). Errors are surfaced inline; the user can dismiss
// either way without affecting the underlying file.

import { headPending, useEnrichStore } from './enrichStore'
import { applyPending, type EnrichFieldSelection } from './enrichRuntime'
import { getEnrichApi } from './enrichApi'
import { zIndex } from '../../../shell/zIndex'

function basename(p: string): string {
  const i = p.lastIndexOf('/')
  return i === -1 ? p : p.slice(i + 1)
}

export function EnrichAcceptGate() {
  const pendingMap = useEnrichStore((s) => s.pending)
  const applying = useEnrichStore((s) => s.applying)
  const error = useEnrichStore((s) => s.error)
  const dismiss = useEnrichStore((s) => s.dismiss)
  const dismissAll = useEnrichStore((s) => s.dismissAll)

  const pending = headPending({ pending: pendingMap })
  const queueSize = pendingMap.size

  if (!pending && !error) return null

  const onAccept = (fields: EnrichFieldSelection = 'all') => {
    const api = getEnrichApi()
    void applyPending(api, fields)
  }

  const hasTags = (pending?.tags.length ?? 0) > 0
  const hasSummary = (pending?.summary ?? '').length > 0
  // Per-field buttons appear only when both tags AND summary are
  // proposed — otherwise "Apply" already does the right thing for
  // whichever single field is populated.
  const showFieldButtons = pending != null && hasTags && hasSummary

  return (
    <div
      role="dialog"
      aria-label="Enrichment proposal"
      style={{
        position: 'fixed',
        right: 16,
        bottom: 16,
        width: 360,
        maxWidth: 'calc(100vw - 32px)',
        background: 'var(--bg-elevated, var(--background-primary))',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 8,
        boxShadow: '0 6px 24px rgba(0,0,0,0.18)',
        padding: 12,
        fontFamily: 'var(--font-interface)',
        fontSize: 13,
        color: 'var(--text-normal)',
        zIndex: zIndex.overlayFloating,
      }}
    >
      {pending && (
        <>
          <div
            style={{
              display: 'flex',
              alignItems: 'baseline',
              justifyContent: 'space-between',
              marginBottom: 6,
            }}
          >
            <span style={{ fontWeight: 600 }}>
              Enrich {basename(pending.path)}?
            </span>
            {queueSize > 1 && (
              <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                +{queueSize - 1} more
              </span>
            )}
          </div>
          {pending.summary && (
            <div style={{ marginBottom: 6, color: 'var(--text-muted)' }}>
              {pending.summary}
            </div>
          )}
          {pending.tags.length > 0 && (
            <div style={{ marginBottom: 6 }}>
              <span style={{ color: 'var(--text-muted)' }}>tags:</span>{' '}
              {pending.tags.map((t, i) => (
                <span
                  key={t}
                  style={{
                    display: 'inline-block',
                    marginRight: 4,
                    padding: '1px 6px',
                    background: 'var(--interactive-accent-soft)',
                    borderRadius: 4,
                    fontSize: 12,
                  }}
                >
                  #{t}
                  {i < pending.tags.length - 1 ? '' : ''}
                </span>
              ))}
            </div>
          )}
          {pending.related.length > 0 && (
            <div style={{ marginBottom: 8, fontSize: 12, color: 'var(--text-muted)' }}>
              related: {pending.related.join(', ')}
            </div>
          )}
        </>
      )}
      {error && (
        <div
          role="alert"
          style={{
            marginBottom: 8,
            padding: 6,
            background: 'var(--bg-warning, rgba(255,0,0,0.06))',
            borderRadius: 4,
            color: 'var(--fg-warning, var(--text-normal))',
            fontSize: 12,
          }}
        >
          {error}
        </div>
      )}
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        {queueSize > 1 && (
          <button
            type="button"
            onClick={() => dismissAll()}
            disabled={applying}
            style={{
              padding: '4px 10px',
              background: 'transparent',
              border: '1px solid var(--background-modifier-border)',
              borderRadius: 4,
              cursor: applying ? 'not-allowed' : 'pointer',
              color: 'var(--text-muted)',
              marginRight: 'auto',
            }}
          >
            Dismiss all
          </button>
        )}
        <button
          type="button"
          onClick={() => (pending ? dismiss(pending.path) : dismissAll())}
          disabled={applying}
          style={{
            padding: '4px 10px',
            background: 'transparent',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 4,
            cursor: applying ? 'not-allowed' : 'pointer',
            color: 'var(--text-normal)',
          }}
        >
          Dismiss
        </button>
        {showFieldButtons && (
          <>
            <button
              type="button"
              onClick={() => onAccept('tags')}
              disabled={applying}
              title="Apply only the suggested tags; leave summary and related untouched."
              style={{
                padding: '4px 10px',
                background: 'transparent',
                color: 'var(--text-normal)',
                border: '1px solid var(--background-modifier-border)',
                borderRadius: 4,
                cursor: applying ? 'not-allowed' : 'pointer',
              }}
              data-testid="enrich-accept-tags"
            >
              Tags only
            </button>
            <button
              type="button"
              onClick={() => onAccept('summary')}
              disabled={applying}
              title="Apply only the suggested summary; leave tags and related untouched."
              style={{
                padding: '4px 10px',
                background: 'transparent',
                color: 'var(--text-normal)',
                border: '1px solid var(--background-modifier-border)',
                borderRadius: 4,
                cursor: applying ? 'not-allowed' : 'pointer',
              }}
              data-testid="enrich-accept-summary"
            >
              Summary only
            </button>
          </>
        )}
        {pending && (
          <button
            type="button"
            onClick={() => onAccept('all')}
            disabled={applying}
            style={{
              padding: '4px 10px',
              background: 'var(--interactive-accent)',
              color: 'white',
              border: 'none',
              borderRadius: 4,
              cursor: applying ? 'not-allowed' : 'pointer',
            }}
            data-testid="enrich-accept-all"
          >
            {applying ? 'Applying…' : showFieldButtons ? 'Apply all' : 'Apply'}
          </button>
        )}
      </div>
    </div>
  )
}
