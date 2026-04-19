import { useEditorStore } from './editorStore'

interface EditorViewProps {
  onRetry: () => void
}

/**
 * Read-only viewer for the currently-open file. Renders into the
 * `editorArea` slot.
 *
 * Layout: a ~36px top strip (file name + forge-relative path) above a
 * scrollable `<pre>` body. Empty, loading, and error states take the
 * full area with centred messaging. Tabs land in a follow-up commit —
 * the top strip is the proto-tab that will expand into a tab bar.
 */
export function EditorView({ onRetry }: EditorViewProps) {
  const openFile = useEditorStore((s) => s.openFile)
  const loading = useEditorStore((s) => s.loading)
  const error = useEditorStore((s) => s.error)

  const rootStyle: React.CSSProperties = {
    display: 'flex',
    flexDirection: 'column',
    width: '100%',
    height: '100%',
    background: 'var(--bg)',
    color: 'var(--fg)',
    fontFamily: 'var(--f-ui)',
    fontSize: 'var(--ui-size, 13px)',
    overflow: 'hidden',
  }

  const centredStyle: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '100%',
    height: '100%',
  }

  if (error) {
    return (
      <div style={rootStyle}>
        <div style={centredStyle}>
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
            <div style={{ color: 'var(--risk)', maxWidth: 480, textAlign: 'center' }}>
              {error}
            </div>
            <button
              onClick={onRetry}
              style={{
                background: 'var(--bg-raised)',
                color: 'var(--fg)',
                border: '1px solid var(--line-soft)',
                borderRadius: 'var(--r, 6px)',
                padding: '6px 14px',
                fontFamily: 'var(--f-ui)',
                fontSize: 'var(--ui-size, 13px)',
                cursor: 'pointer',
              }}
            >
              Retry
            </button>
          </div>
        </div>
      </div>
    )
  }

  if (loading) {
    return (
      <div style={rootStyle}>
        <div style={{ ...centredStyle, color: 'var(--fg-dim)' }}>Loading…</div>
      </div>
    )
  }

  if (!openFile) {
    return (
      <div style={rootStyle}>
        <div style={{ ...centredStyle, color: 'var(--fg-dim)' }}>
          Select a file to view
        </div>
      </div>
    )
  }

  return (
    <div style={rootStyle}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          height: 36,
          flex: '0 0 36px',
          padding: '0 16px',
          borderBottom: '1px solid var(--line-soft)',
          background: 'var(--bg-raised)',
          gap: 12,
        }}
      >
        <span
          style={{
            color: 'var(--fg)',
            fontWeight: 500,
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
          }}
        >
          {openFile.name}
        </span>
        <span
          style={{
            color: 'var(--fg-muted)',
            fontSize: 'calc(var(--ui-size, 13px) - 1px)',
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            direction: 'rtl',
            textAlign: 'right',
            minWidth: 0,
          }}
          title={openFile.relpath}
        >
          {openFile.relpath}
        </span>
      </div>
      <div style={{ flex: '1 1 auto', overflow: 'auto' }}>
        <pre
          style={{
            margin: 0,
            padding: '16px 20px',
            fontFamily: 'var(--f-mono)',
            fontSize: 'var(--ui-size, 13px)',
            color: 'var(--fg)',
            whiteSpace: 'pre-wrap',
            wordWrap: 'break-word',
          }}
        >
          {openFile.content}
        </pre>
      </div>
    </div>
  )
}
