import { createElement } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useLayoutStore } from '../../../stores/layoutStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { Icon } from '../../../icons'
import { getRegistry } from '../../../host/shellRegistry'

function CollapseIcon() {
  return (
    <svg width={14} height={14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M5 12h14" />
    </svg>
  )
}

function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] ?? trimmed
}

function SidebarIconBtn({
  label,
  onClick,
  children,
}: {
  label: string
  onClick?: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      onMouseEnter={(e) => {
        ;(e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
        ;(e.currentTarget as HTMLButtonElement).style.color = 'var(--fg)'
      }}
      onMouseLeave={(e) => {
        ;(e.currentTarget as HTMLButtonElement).style.background = 'transparent'
        ;(e.currentTarget as HTMLButtonElement).style.color = 'var(--fg-muted)'
      }}
      style={{
        width: 22,
        height: 22,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        borderRadius: 'var(--r)',
        flexShrink: 0,
      }}
    >
      {children}
    </button>
  )
}

export function SidebarHost() {
  const activeViewId = useLayoutStore((s) => s.sidebar.activeView)
  const entries = useSlotStore((s) => s.slots.sidebarContent)
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const openWorkspace = useWorkspaceStore((s) => s.open)

  if (!activeViewId) return null
  const match = entries.find((e) => e.id === activeViewId)
  if (!match) return null

  const workspaceName = rootPath ? basename(rootPath) : null

  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar)

  const openSettings = () => {
    const reg = getRegistry()
    reg?.commands.execute('workbench.action.openSettings')
  }

  return (
    <div
      style={{
        height: '100%',
        width: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {/* Workspace header */}
      <div
        style={{
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 2,
          padding: '0 6px 0 12px',
          height: 34,
          borderBottom: '1px solid var(--line-soft)',
        }}
      >
        <span
          style={{
            flex: 1,
            fontSize: 11,
            fontWeight: 700,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            color: 'var(--fg-muted)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            userSelect: 'none',
          }}
        >
          {workspaceName ?? 'No workspace'}
        </span>
        <SidebarIconBtn label="New file">
          <Icon name="plus" size={14} />
        </SidebarIconBtn>
        <SidebarIconBtn label="Open workspace" onClick={() => openWorkspace()}>
          <Icon name="folderOpen" size={14} />
        </SidebarIconBtn>
        <SidebarIconBtn label="Collapse sidebar" onClick={() => toggleSidebar()}>
          <CollapseIcon />
        </SidebarIconBtn>
      </div>

      {/* Active view content */}
      <div
        style={{
          flex: 1,
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
          minHeight: 0,
        }}
      >
        {createElement(match.component)}
      </div>

      {/* Vault footer */}
      <div
        style={{
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '0 6px 0 10px',
          height: 32,
          borderTop: '1px solid var(--line-soft)',
          fontSize: 12,
          color: 'var(--fg-muted)',
        }}
      >
        <Icon name="folder" size={13} />
        <span
          style={{
            flex: 1,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            userSelect: 'none',
          }}
        >
          {workspaceName ?? 'No workspace'}
        </span>
        <SidebarIconBtn label="Help">
          <svg
            width={14}
            height={14}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={1.75}
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="9" />
            <path d="M9.5 9a2.5 2.5 0 0 1 5 .6c0 2-2.5 2.4-2.5 4" />
            <circle cx="12" cy="17.5" r=".5" fill="currentColor" />
          </svg>
        </SidebarIconBtn>
        <SidebarIconBtn label="Settings" onClick={openSettings}>
          <Icon name="settings" size={14} />
        </SidebarIconBtn>
      </div>
    </div>
  )
}
