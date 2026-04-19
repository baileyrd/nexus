import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useEditorStore } from '../editor/editorStore'
import { useLayoutStore } from '../../../stores/layoutStore'
import { Icon } from '../../../icons'
import { getApi } from './runtime'

const SEARCH_FOCUS_COMMAND = 'nexus.search.focus'

function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

function fileExt(name: string): string | null {
  const i = name.lastIndexOf('.')
  if (i <= 0 || i === name.length - 1) return null
  return name.slice(i + 1).toLowerCase()
}

/** Approximate word count for the breadcrumb badge. Splits on
 *  whitespace which matches how the design bundle renders the figure
 *  (rough; not character-accurate). Returns null for non-textual or
 *  empty content so the badge stays out of the DOM. */
function wordCount(content: string | null | undefined): number | null {
  if (!content) return null
  const trimmed = content.trim()
  if (!trimmed) return null
  return trimmed.split(/\s+/).length
}

const baseControlStyle: React.CSSProperties = {
  width: 40,
  height: 36,
  background: 'transparent',
  border: 'none',
  color: 'var(--fg-muted)',
  cursor: 'pointer',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 0,
  transition: 'background 0.08s, color 0.08s',
}

function ControlButton({
  onClick,
  label,
  closeAccent,
  children,
}: {
  onClick: () => void
  label: string
  closeAccent?: boolean
  children: React.ReactNode
}) {
  const [hover, setHover] = useState(false)
  // Windows convention: close button hovers to red (literal so it lands
  // unambiguously regardless of theme-token availability); other controls
  // use a subtle raised background from the token palette.
  const hoverBg = closeAccent ? '#e81123' : 'var(--bg-hover)'
  const hoverFg = closeAccent ? '#ffffff' : 'var(--fg)'
  const style: React.CSSProperties = {
    ...baseControlStyle,
    background: hover ? hoverBg : 'transparent',
    color: hover ? hoverFg : 'var(--fg-muted)',
  }
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      aria-label={label}
      title={label}
      style={style}
    >
      {children}
    </button>
  )
}

function MinimizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <rect x="0" y="4.5" width="10" height="1" fill="currentColor" />
    </svg>
  )
}

function MaximizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
    </svg>
  )
}

function RestoreIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <rect x="2.5" y="0.5" width="7" height="7" fill="none" stroke="currentColor" />
      <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" />
    </svg>
  )
}

function CloseIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" />
      <line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" />
    </svg>
  )
}

/**
 * Square icon button used by the left cluster + right cluster.
 * Visually distinct from the Windows controls (smaller width, hover
 * uses raised bg rather than the platform highlight).
 */
function ClusterButton({
  onClick,
  label,
  active,
  children,
}: {
  onClick: () => void
  label: string
  active?: boolean
  children: React.ReactNode
}) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      aria-label={label}
      title={label}
      aria-pressed={active}
      style={{
        width: 28,
        height: 26,
        padding: 0,
        background: active ? 'var(--bg)' : hover ? 'var(--bg-hover)' : 'transparent',
        border: 0,
        color: active ? 'var(--fg)' : hover ? 'var(--fg)' : 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        borderRadius: 'var(--r)',
        transition: 'background 0.08s, color 0.08s',
      }}
    >
      {children}
    </button>
  )
}

export function TitleBar() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const openWorkspace = useWorkspaceStore((s) => s.open)
  const tabs = useEditorStore((s) => s.tabs)
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const rightPanelVisible = useLayoutStore((s) => s.rightPanel.visible)
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel)
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    const w = getCurrentWindow()
    let unlisten: (() => void) | undefined
    let cancelled = false
    ;(async () => {
      try {
        const current = await w.isMaximized()
        if (!cancelled) setMaximized(current)
        unlisten = await w.onResized(async () => {
          const now = await w.isMaximized()
          setMaximized(now)
        })
      } catch (err) {
        console.warn('[nexus.titleBar] failed to wire maximize listener:', err)
      }
    })()
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const minimize = () => getCurrentWindow().minimize()
  const toggleMaximize = () => getCurrentWindow().toggleMaximize()
  const close = () => getCurrentWindow().close()

  const focusSearch = () => {
    // Same dispatcher path the command palette uses internally. The
    // search plugin's command both raises the sidebar view and
    // focuses the input.
    const api = getApi()
    if (!api) return
    void api.commands.execute(SEARCH_FOCUS_COMMAND)
  }

  const activeTab = tabs.find((t) => t.relpath === activeRelpath) ?? null

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        height: '100%',
        width: '100%',
        userSelect: 'none',
        color: 'var(--fg-muted)',
        fontSize: 'var(--ui-size, 12px)',
        gap: 4,
        paddingLeft: 6,
      }}
    >
      <LeftCluster
        rootPath={rootPath}
        onOpen={() => openWorkspace()}
        onSearch={focusSearch}
      />

      <Breadcrumb rootPath={rootPath} activeRelpath={activeRelpath} activeContent={activeTab?.content ?? null} />

      {/* Drag region: middle spacer only. Keeping interactive buttons out of
          the drag region avoids eaten pointer events on Windows/Tauri 2. */}
      <div data-tauri-drag-region style={{ flex: 1, height: '100%' }} />

      <RightCluster
        rightPanelVisible={rightPanelVisible}
        onToggleRightPanel={toggleRightPanel}
      />

      <ControlButton onClick={minimize} label="Minimize">
        <MinimizeIcon />
      </ControlButton>
      <ControlButton
        onClick={toggleMaximize}
        label={maximized ? 'Restore' : 'Maximize'}
      >
        {maximized ? <RestoreIcon /> : <MaximizeIcon />}
      </ControlButton>
      <ControlButton onClick={close} label="Close" closeAccent>
        <CloseIcon />
      </ControlButton>
    </div>
  )
}

function LeftCluster({
  rootPath,
  onOpen,
  onSearch,
}: {
  rootPath: string | null
  onOpen: () => void
  onSearch: () => void
}) {
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 2,
        padding: '0 4px',
        background: 'var(--bg-raised)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        height: 28,
      }}
    >
      <ClusterButton
        onClick={onOpen}
        label={rootPath ? `Workspace · ${rootPath}` : 'Open workspace'}
      >
        <Icon name="folder" size={14} />
      </ClusterButton>
      <ClusterButton onClick={onSearch} label="Focus search">
        <Icon name="search" size={14} />
      </ClusterButton>
    </div>
  )
}

function Breadcrumb({
  rootPath,
  activeRelpath,
  activeContent,
}: {
  rootPath: string | null
  activeRelpath: string | null
  activeContent: string | null
}) {
  const workspaceName = rootPath ? basename(rootPath) : null
  const fileName = activeRelpath ? basename(activeRelpath) : null
  const ext = fileName ? fileExt(fileName) : null
  const words = activeContent ? wordCount(activeContent) : null

  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        marginLeft: 8,
        minWidth: 0,
        overflow: 'hidden',
      }}
    >
      <span
        title={rootPath ? 'Forge synced' : 'No workspace open'}
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: '50%',
          background: rootPath ? 'var(--ok)' : 'var(--fg-dim)',
          flex: '0 0 auto',
          boxShadow: rootPath ? '0 0 4px var(--ok)' : 'none',
        }}
      />
      {workspaceName ? (
        <span
          title={rootPath ?? undefined}
          style={{
            color: 'var(--fg-muted)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            flex: '0 1 auto',
          }}
        >
          {workspaceName}
        </span>
      ) : (
        <span style={{ color: 'var(--fg-dim)', fontStyle: 'italic' }}>No workspace</span>
      )}
      {fileName ? (
        <>
          <span style={{ color: 'var(--fg-dim)', flex: '0 0 auto' }}>/</span>
          <span
            title={activeRelpath ?? undefined}
            style={{
              color: 'var(--fg)',
              fontWeight: 500,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              minWidth: 0,
            }}
          >
            {fileName}
          </span>
          {ext || words !== null ? (
            <span
              style={{
                marginLeft: 4,
                color: 'var(--fg-dim)',
                fontFamily: 'var(--f-mono, monospace)',
                fontSize: 10,
                flex: '0 0 auto',
              }}
            >
              {[ext, words !== null ? `${words.toLocaleString()}w` : null]
                .filter(Boolean)
                .join(' · ')}
            </span>
          ) : null}
        </>
      ) : null}
    </div>
  )
}

function RightCluster({
  rightPanelVisible,
  onToggleRightPanel,
}: {
  rightPanelVisible: boolean
  onToggleRightPanel: () => void
}) {
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 2,
        padding: '0 4px',
        height: 28,
        marginRight: 6,
      }}
    >
      <ClusterButton
        onClick={onToggleRightPanel}
        label={rightPanelVisible ? 'Hide right panel' : 'Show right panel'}
        active={rightPanelVisible}
      >
        <Icon name="panel" size={14} />
      </ClusterButton>
    </div>
  )
}
