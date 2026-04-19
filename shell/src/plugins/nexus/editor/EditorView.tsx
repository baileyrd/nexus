import { useMemo } from 'react'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { useEditorStore, isDirty, type EditorTab, type EditorTabMode } from './editorStore'
import './markdown.css'

interface EditorViewProps {
  onRetry: (relpath: string) => void
  /**
   * Confirm-then-close entry point shared with the keybinding
   * command handler in index.ts. Prompts only if the tab is dirty;
   * cancelling aborts the close.
   */
  onRequestClose: (relpath: string) => void
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
 * Editor view: tab row with per-tab dirty dot + a mode-toggle button
 * at the right end of the tab row, above a body that renders the
 * active tab either as markdown/<pre> (preview) or as a monospaced
 * textarea (source).
 *
 * Empty, loading, and error states are computed per-tab so a failed
 * load on one tab doesn't bleed into any neighbour.
 */
export function EditorView({ onRetry, onRequestClose }: EditorViewProps) {
  const tabs = useEditorStore((s) => s.tabs)
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const setActive = useEditorStore((s) => s.setActive)
  const setMode = useEditorStore((s) => s.setMode)

  const activeTab = useMemo<EditorTab | null>(
    () => tabs.find((t) => t.relpath === activeRelpath) ?? null,
    [tabs, activeRelpath],
  )

  // Parse markdown once per content change — re-running marked + DOMPurify
  // on every unrelated parent re-render would be needlessly expensive.
  const markdownHtml = useMemo(() => {
    if (!activeTab) return ''
    if (activeTab.loading || activeTab.error) return ''
    if (activeTab.mode !== 'preview') return ''
    if (!isMarkdown(activeTab.name)) return ''
    return renderMarkdown(activeTab.content)
  }, [
    activeTab?.relpath,
    activeTab?.content,
    activeTab?.name,
    activeTab?.loading,
    activeTab?.error,
    activeTab?.mode,
  ])

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
        activeTab={activeTab}
        onSelect={setActive}
        onRequestClose={onRequestClose}
        onToggleMode={() => {
          if (!activeTab) return
          const next: EditorTabMode = activeTab.mode === 'preview' ? 'source' : 'preview'
          setMode(activeTab.relpath, next)
        }}
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
  activeTab: EditorTab | null
  onSelect: (relpath: string) => void
  onRequestClose: (relpath: string) => void
  onToggleMode: () => void
}

function TabBar({ tabs, activeRelpath, activeTab, onSelect, onRequestClose, onToggleMode }: TabBarProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'stretch',
        height: 36,
        flex: '0 0 36px',
        background: 'var(--bg-raised)',
        borderBottom: '1px solid var(--line-soft)',
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'stretch',
          flex: '1 1 auto',
          minWidth: 0,
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
            onRequestClose={onRequestClose}
          />
        ))}
      </div>
      {activeTab ? <ModeToggle mode={activeTab.mode} onClick={onToggleMode} /> : null}
    </div>
  )
}

interface TabItemProps {
  tab: EditorTab
  active: boolean
  onSelect: (relpath: string) => void
  onRequestClose: (relpath: string) => void
}

function TabItem({ tab, active, onSelect, onRequestClose }: TabItemProps) {
  const dirty = isDirty(tab)
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
          display: 'flex',
          alignItems: 'center',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          fontWeight: active ? 500 : 400,
          minWidth: 0,
        }}
      >
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{tab.name}</span>
        {dirty ? (
          <span
            aria-hidden
            title="Unsaved changes"
            style={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: 'var(--fg)',
              marginLeft: 4,
              flexShrink: 0,
            }}
          />
        ) : null}
      </span>
      <CloseButton
        onClick={(e) => {
          e.stopPropagation()
          onRequestClose(tab.relpath)
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

interface ModeToggleProps {
  mode: EditorTabMode
  onClick: () => void
}

/**
 * Right-edge mode toggle. Shows the icon for the action the click
 * will perform: pencil when in preview (click to edit), eye when in
 * source (click to preview). Aria-label mirrors the action.
 */
function ModeToggle({ mode, onClick }: ModeToggleProps) {
  const willEdit = mode === 'preview'
  const label = willEdit ? 'Edit' : 'Preview'

  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
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
        flex: '0 0 32px',
        width: 32,
        height: 32,
        alignSelf: 'center',
        marginRight: 4,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--fg-muted)',
        cursor: 'pointer',
        borderRadius: 'var(--r)',
      }}
    >
      {willEdit ? (
        // Pencil — click to edit (currently in preview)
        <svg
          width={16}
          height={16}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.75}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M12 20 h9 M16.5 3.5 a2.12 2.12 0 0 1 3 3 L7 19 l-4 1 1 -4 z" />
        </svg>
      ) : (
        // Eye — click to preview (currently in source)
        <svg
          width={16}
          height={16}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.75}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M2 12 s3.5 -7 10 -7 s10 7 10 7 s-3.5 7 -10 7 s-10 -7 -10 -7 z" />
          <circle cx={12} cy={12} r={3} />
        </svg>
      )}
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

  if (tab.mode === 'source') {
    return (
      <textarea
        className="nexus-editor-source"
        value={tab.content}
        onChange={(e) =>
          useEditorStore.getState().setContent(tab.relpath, e.target.value)
        }
        spellCheck={false}
        autoCapitalize="off"
      />
    )
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
