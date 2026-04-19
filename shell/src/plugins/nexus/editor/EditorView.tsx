import { useMemo } from 'react'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { useEditorStore } from './editorStore'
import './markdown.css'

interface EditorViewProps {
  onRetry: () => void
}

function isMarkdown(name: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(name)
}

// marked.parse returns string when `async: false`. Sanitize before
// we hand the HTML to React's dangerouslySetInnerHTML — user notes
// aren't hostile, but DOMPurify is cheap insurance.
function renderMarkdown(content: string): string {
  const raw = marked.parse(content, { async: false }) as string
  return DOMPurify.sanitize(raw)
}

/**
 * Read-only viewer for the currently-open file. Renders into the
 * `editorArea` slot.
 *
 * Layout: a ~36px top strip (file name + forge-relative path) above a
 * scrollable body. For `.md` / `.markdown` / `.mdx` files we render
 * sanitized HTML via marked+DOMPurify inside `.nexus-markdown-body`;
 * everything else keeps the monospaced `<pre>` fallback.
 *
 * Empty, loading, and error states take the full area with centred
 * messaging. Tabs land in a follow-up commit — the top strip is the
 * proto-tab that will expand into a tab bar.
 */
export function EditorView({ onRetry }: EditorViewProps) {
  const openFile = useEditorStore((s) => s.openFile)
  const loading = useEditorStore((s) => s.loading)
  const error = useEditorStore((s) => s.error)

  // Parse markdown once per content change — re-running marked + DOMPurify
  // on every unrelated parent re-render would be needlessly expensive.
  const markdownHtml = useMemo(() => {
    if (!openFile) return ''
    if (!isMarkdown(openFile.name)) return ''
    return renderMarkdown(openFile.content)
  }, [openFile?.content, openFile?.name])

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

  const markdown = isMarkdown(openFile.name)

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
        {markdown ? (
          <div
            className="nexus-markdown-body"
            dangerouslySetInnerHTML={{ __html: markdownHtml }}
          />
        ) : (
          <pre className="nexus-editor-raw">{openFile.content}</pre>
        )}
      </div>
    </div>
  )
}
