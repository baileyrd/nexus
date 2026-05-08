// BL-067 Phase 1 — View Builder panel.
//
// A read-only-first surface for managing named workspace layouts.
// Three sections:
//
//   1. Current layout — renders the live tree (`workspace.layoutSnapshot()`)
//      as an indented summary so the user can see what's present
//      before they save it.
//   2. Saved layouts — list of `<forge>/.forge/layouts/*.layout.json`
//      files with apply / delete actions, plus an inline "Save current
//      as…" form.
//   3. Registered view types — read-only catalog of every viewType
//      currently registered in `ViewRegistry`. The drag-drop /
//      "add panel" surface is a deferred BL-067 follow-up.

import { useCallback, useEffect, useState, type ReactElement } from 'react'

import { useViewStore, workspace } from '../../../workspace'
import type {
  SerializedFloating,
  SerializedNode,
  SerializedSplit,
  SerializedTabs,
} from '../../../workspace/types'
import { useLayoutsStore, normaliseName } from './layoutsStore'

interface Props {
  /** Apply the saved layout (by name). The plugin wires this to a
   *  `loadLayout` + `applySnapshot` round-trip. */
  onApply: (name: string) => Promise<void>
  /** Save the current layout under `name`. */
  onSave: (name: string) => Promise<void>
  /** Delete the named layout from disk. */
  onDelete: (name: string) => Promise<void>
  /** Refresh the saved-layouts list. */
  onRefresh: () => void
}

export function ViewBuilderView({
  onApply,
  onSave,
  onDelete,
  onRefresh,
}: Props): ReactElement {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        fontFamily: 'var(--font-interface)',
        color: 'var(--text-normal)',
        background: 'var(--background-primary)',
        overflowY: 'auto',
        padding: '8px 10px',
        gap: 16,
      }}
    >
      <CurrentLayoutSection />
      <SavedLayoutsSection
        onApply={onApply}
        onSave={onSave}
        onDelete={onDelete}
        onRefresh={onRefresh}
      />
      <ViewTypesSection />
    </div>
  )
}

// ── Current layout ──────────────────────────────────────────────────────────

function CurrentLayoutSection(): ReactElement {
  // Pull a snapshot at every render. Cheap (clone of in-memory tree)
  // and saves us a subscription wire-up; the panel re-renders when
  // any of its existing zustand selectors fires.
  const snapshot = workspace.layoutSnapshot()
  return (
    <section>
      <h3 style={sectionHeading}>Current layout</h3>
      <div
        style={{
          fontSize: '0.85em',
          background: 'var(--background-secondary)',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 4,
          padding: '6px 8px',
          fontFamily: 'var(--font-monospace)',
          lineHeight: 1.5,
        }}
      >
        <NodeBlock label="main" node={snapshot.main} />
        <NodeBlock label="left" node={snapshot.left} />
        <NodeBlock label="right" node={snapshot.right} />
        {snapshot.bottom && <NodeBlock label="bottom" node={snapshot.bottom} />}
        {snapshot.floating && snapshot.floating.length > 0 && (
          <FloatingList floating={snapshot.floating} />
        )}
      </div>
    </section>
  )
}

function NodeBlock({ label, node }: { label: string; node: SerializedNode }): ReactElement {
  return (
    <div>
      <div style={{ fontWeight: 600 }}>{label}</div>
      <NodeTree node={node} depth={1} />
    </div>
  )
}

function NodeTree({ node, depth }: { node: SerializedNode; depth: number }): ReactElement {
  const indent = { paddingLeft: depth * 12 }
  if (node.kind === 'split') {
    const split = node as SerializedSplit
    const sideLabel = split.side ? ` [side=${split.side}${split.collapsed ? ' (collapsed)' : ''}]` : ''
    return (
      <div style={indent}>
        <span style={{ color: 'var(--text-muted)' }}>
          split ({split.direction}){sideLabel}
        </span>
        {split.children.map((child, i) => (
          <NodeTree key={`${child.kind}-${i}`} node={child} depth={depth + 1} />
        ))}
      </div>
    )
  }
  if (node.kind === 'tabs') {
    const tabs = node as SerializedTabs
    return (
      <div style={indent}>
        <span style={{ color: 'var(--text-muted)' }}>
          tabs ({tabs.leaves.length})
        </span>
        {tabs.leaves.map((leaf, i) => (
          <div key={leaf.id} style={{ paddingLeft: (depth + 1) * 12 }}>
            <span
              style={{
                color: i === tabs.activeIndex ? 'var(--interactive-accent)' : 'inherit',
              }}
            >
              {leaf.viewState.type}
              {i === tabs.activeIndex ? ' ●' : ''}
            </span>
          </div>
        ))}
      </div>
    )
  }
  if (node.kind === 'root') {
    return <NodeTree node={node.child} depth={depth} />
  }
  if (node.kind === 'floating') {
    return <NodeTree node={node.child} depth={depth} />
  }
  return <div style={indent}>leaf {(node as { id: string }).id}</div>
}

function FloatingList({ floating }: { floating: SerializedFloating[] }): ReactElement {
  return (
    <div>
      <div style={{ fontWeight: 600, marginTop: 6 }}>floating ({floating.length})</div>
      {floating.map((fw) => (
        <NodeTree key={fw.id} node={fw} depth={1} />
      ))}
    </div>
  )
}

// ── Saved layouts ───────────────────────────────────────────────────────────

function SavedLayoutsSection({
  onApply,
  onSave,
  onDelete,
  onRefresh,
}: Props): ReactElement {
  const layouts = useLayoutsStore((s) => s.layouts)
  const loading = useLayoutsStore((s) => s.loading)
  const error = useLayoutsStore((s) => s.error)
  const [draftName, setDraftName] = useState('')
  const [busy, setBusy] = useState<string | null>(null)
  const [feedback, setFeedback] = useState<string | null>(null)
  const [feedbackKind, setFeedbackKind] = useState<'ok' | 'error'>('ok')

  const showFeedback = useCallback((message: string, kind: 'ok' | 'error') => {
    setFeedback(message)
    setFeedbackKind(kind)
  }, [])

  const handleSave = useCallback(async () => {
    let name: string
    try {
      name = normaliseName(draftName)
    } catch (err) {
      showFeedback(err instanceof Error ? err.message : String(err), 'error')
      return
    }
    setBusy(`save:${name}`)
    try {
      await onSave(name)
      setDraftName('')
      showFeedback(`Saved "${name}"`, 'ok')
    } catch (err) {
      showFeedback(err instanceof Error ? err.message : String(err), 'error')
    } finally {
      setBusy(null)
    }
  }, [draftName, onSave, showFeedback])

  const handleApply = useCallback(
    async (name: string) => {
      setBusy(`apply:${name}`)
      try {
        await onApply(name)
        showFeedback(`Switched to "${name}"`, 'ok')
      } catch (err) {
        showFeedback(err instanceof Error ? err.message : String(err), 'error')
      } finally {
        setBusy(null)
      }
    },
    [onApply, showFeedback],
  )

  const handleDelete = useCallback(
    async (name: string) => {
      setBusy(`del:${name}`)
      try {
        await onDelete(name)
        showFeedback(`Deleted "${name}"`, 'ok')
      } catch (err) {
        showFeedback(err instanceof Error ? err.message : String(err), 'error')
      } finally {
        setBusy(null)
      }
    },
    [onDelete, showFeedback],
  )

  return (
    <section>
      <header style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
        <h3 style={sectionHeading}>Saved layouts</h3>
        <button
          type="button"
          onClick={onRefresh}
          style={subtleButton}
          aria-label="Refresh"
        >
          Refresh
        </button>
      </header>

      <form
        onSubmit={(e) => {
          e.preventDefault()
          void handleSave()
        }}
        style={{ display: 'flex', gap: 6, marginBottom: 8 }}
      >
        <input
          type="text"
          placeholder="Save current as… (e.g. Focus, Research, Dev)"
          value={draftName}
          onChange={(e) => setDraftName(e.target.value)}
          maxLength={80}
          style={{
            flex: 1,
            padding: '4px 6px',
            background: 'var(--background-primary)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 3,
            color: 'inherit',
            font: 'inherit',
          }}
        />
        <button
          type="submit"
          disabled={!draftName.trim() || busy != null}
          style={primaryButton}
        >
          Save
        </button>
      </form>

      {feedback != null && (
        <div
          style={{
            padding: '4px 6px',
            marginBottom: 6,
            fontSize: '0.85em',
            color: feedbackKind === 'error' ? 'var(--text-error, #d04040)' : 'var(--text-normal)',
            background: 'var(--background-secondary)',
            borderRadius: 3,
          }}
        >
          {feedback}
        </div>
      )}

      {error != null && (
        <div style={{ color: 'var(--text-error, #d04040)', fontSize: '0.85em' }}>
          {error}
        </div>
      )}

      {layouts.length === 0 ? (
        <div style={{ color: 'var(--text-muted)', fontSize: '0.85em' }}>
          {loading ? 'Loading…' : 'No saved layouts yet.'}
        </div>
      ) : (
        <ul style={{ listStyle: 'none', margin: 0, padding: 0 }}>
          {layouts.map((row) => (
            <li
              key={row.relpath}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                padding: '3px 0',
                borderBottom: '1px solid var(--background-modifier-border)',
              }}
            >
              <span style={{ flex: 1 }}>{row.name}</span>
              <button
                type="button"
                onClick={() => void handleApply(row.name)}
                disabled={busy != null}
                style={subtleButton}
              >
                Apply
              </button>
              <button
                type="button"
                onClick={() => void handleDelete(row.name)}
                disabled={busy != null}
                style={dangerButton}
              >
                Delete
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

// ── View types catalog ──────────────────────────────────────────────────────

function ViewTypesSection(): ReactElement {
  const creators = useViewStore((s) => s.creators)
  // Stable sort so the list isn't insertion-ordered on every render.
  const types = [...creators.keys()].sort()
  return (
    <section>
      <h3 style={sectionHeading}>Registered views ({types.length})</h3>
      <div style={{ fontSize: '0.85em', color: 'var(--text-muted)', marginBottom: 4 }}>
        Read-only inventory. Drag-to-add and per-panel options land in a
        future pass.
      </div>
      <ul
        style={{
          listStyle: 'none',
          margin: 0,
          padding: 0,
          fontFamily: 'var(--font-monospace)',
          fontSize: '0.85em',
        }}
      >
        {types.map((t) => (
          <li
            key={t}
            style={{
              padding: '2px 0',
              borderBottom: '1px solid var(--background-modifier-border)',
            }}
          >
            {t}
          </li>
        ))}
      </ul>
    </section>
  )
}

// ── Helper that lets the leaf wrapper trigger an initial refresh ────────────

/** Hook the panel uses to call `onRefresh` once on mount. Pulled out
 *  as a hook so it can be tested without instantiating React's
 *  effect graph. */
export function useInitialRefresh(onRefresh: () => void): void {
  useEffect(() => {
    onRefresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
}

// ── Style scraps ────────────────────────────────────────────────────────────

const sectionHeading: React.CSSProperties = {
  margin: '0 0 6px 0',
  fontSize: '0.9em',
  fontWeight: 600,
}

const subtleButton: React.CSSProperties = {
  background: 'transparent',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 3,
  padding: '2px 8px',
  fontSize: '0.8em',
  color: 'inherit',
  cursor: 'pointer',
}

const primaryButton: React.CSSProperties = {
  background: 'var(--interactive-accent)',
  border: 0,
  borderRadius: 3,
  padding: '4px 10px',
  fontSize: '0.85em',
  color: 'var(--text-on-accent, #fff)',
  cursor: 'pointer',
}

const dangerButton: React.CSSProperties = {
  background: 'transparent',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 3,
  padding: '2px 8px',
  fontSize: '0.8em',
  color: 'var(--text-error, #d04040)',
  cursor: 'pointer',
}
