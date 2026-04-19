import { useMemo } from 'react'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { useEditorStore, type EditorTab } from './editorStore'
import './markdown.css'

interface EditorViewProps {
  onRetry: (relpath: string) => void
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
 * Layout: a 36px horizontal tab row above a scrollable body. Each
 * tab reports its own filename, is click-to-activate, and carries a
 * small × close button. The body area reads the active tab and
 * renders its `content` — markdown-vs-pre branch is keyed on the
 * active tab's name / content.
 *
 * Empty, loading, and error states are computed per-tab so a failed
 * load on one tab doesn't bleed into any neighbour.
 */
export function EditorView({ onRetry }: EditorViewProps) {
  const tabs = useEditorStore((s) => s.tabs)
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const setActive = useEditorStore((s) => s.setActive)
  const closeTab = useEditorStore((s) => s.closeTab)

  const activeTab = useMemo<EditorTab | null>(
    () => tabs.find((t) => t.relpath === activeRelpath) ?? null,
    [tabs, activeRelpath],
  )

  // Parse markdown once per content change — re-running marked + DOMPurify
  // on every unrelated parent re-render would be needlessly expensive.
  const markdownHtml = useMemo(() => {
    if (!activeTab) return ''
    if (activeTab.loading || activeTab.error) return ''
    if (!isMarkdown(activeTab.name)) return ''
    return renderMarkdown(activeTab.content)
  }, [activeTab?.relpath, activeTab?.content, activeTab?.name, activeTab?.loading, activeTab?.error])

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

  if (tabs.length === 0) {
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
      <TabBar
        tabs={tabs}
        activeRelpath={activeRelpath}
        onSelect={setActive}
        onClose={closeTab}
      />
      <div style={{ flex: '1 1 auto', overflow: 'auto' }}>
        {activeTab ? (
          <TabBody tab={activeTab} markdownHtml={markdownHtml} onRetry={onRetry} />
        ) : (
          <div style={{ ...centredStyle, color: 'var(--fg-dim)' }}>
            Select a tab
          </div>
        )}
      </div>
    </div>
  )
}

interface TabBarProps {
  tabs: EditorTab[]
  activeRelpath: string | null
  onSelect: (relpath: string) => void
  onClose: (relpath: string) => void
}

function TabBar({ tabs, activeRelpath, onSelect, onClose }: TabBarProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'stretch',
        height: 36,
        flex: '0 0 36px',
        background: 'var(--bg-raised)',
        borderBottom: '1px solid var(--line-soft)',
        overflowX: 'auto',
        overflowY: 'hidden',
      }}
    >
      {tabs.map((tab) => (
        <TabItem
          key={tab.relpath}
          tab={tab}
          active={tab.relpath === activeRelpath}
          onSelect={onSelect}
          onClose={onClose}
        />
      ))}
    </div>
  )
}

interface TabItemProps {
  tab: EditorTab
  active: boolean
  onSelect: (relpath: string) => void
  onClose: (relpath: string) => void
}

function TabItem({ tab, active, onSelect, onClose }: TabItemProps) {
  const style: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '0 10px',
    height: '100%',
    borderRight: '1px solid var(--line-soft)',
    cursor: 'pointer',
    whiteSpace: 'nowrap',
    flexShrink: 0,
    maxWidth: 220,
    minWidth: 80,
    background: active ? 'var(--bg)' : 'transparent',
    color: active ? 'var(--fg)' : 'var(--fg-muted)',
  }

  return (
    <div
      role="tab"
      aria-selected={active}
      title={tab.relpath}
      style={style}
      onClick={() => onSelect(tab.relpath)}
      onMouseEnter={(e) => {
        if (!active) {
          (e.currentTarget as HTMLDivElement).style.background = 'var(--bg-hover)'
        }
      }}
      onMouseLeave={(e) => {
        if (!active) {
          (e.currentTarget as HTMLDivElement).style.background = 'transparent'
        }
      }}
    >
      <span
        style={{
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          fontWeight: active ? 500 : 400,
        }}
      >
        {tab.name}
      </span>
      <CloseButton
        onClick={(e) => {
          e.stopPropagation()
          onClose(tab.relpath)
        }}
      />
    </div>
  )
}

interface CloseButtonProps {
  onClick: (e: React.MouseEvent) => void
}

function CloseButton({ onClick }: CloseButtonProps) {
  return (
    <button
      type="button"
      aria-label="Close"
      onClick={onClick}
      onMouseDown={(e) => e.stopPropagation()}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'transparent'
      }}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: 16,
        height: 16,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'inherit',
        cursor: 'pointer',
        borderRadius: 'var(--r)',
      }}
    >
      <svg
        width={12}
        height={12}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.75}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M18 6 6 18" />
        <path d="m6 6 12 12" />
      </svg>
    </button>
  )
}

interface TabBodyProps {
  tab: EditorTab
  markdownHtml: string
  onRetry: (relpath: string) => void
}

function TabBody({ tab, markdownHtml, onRetry }: TabBodyProps) {
  const centredStyle: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '100%',
    height: '100%',
  }

  if (tab.error) {
    return (
      <div style={centredStyle}>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
          <div style={{ color: 'var(--risk)', maxWidth: 480, textAlign: 'center' }}>
            {tab.error}
          </div>
          <button
            onClick={() => onRetry(tab.relpath)}
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
    )
  }

  if (tab.loading) {
    return <div style={{ ...centredStyle, color: 'var(--fg-dim)' }}>Loading…</div>
  }

  if (isMarkdown(tab.name)) {
    return (
      <div
        className="nexus-markdown-body"
        dangerouslySetInnerHTML={{ __html: markdownHtml }}
      />
    )
  }

  return <pre className="nexus-editor-raw">{tab.content}</pre>
}
