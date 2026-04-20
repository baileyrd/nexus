import { createElement, useMemo } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useActivityBarStore } from '../activityBar/activityBarStore'
import { Icon } from '../../../icons'
import { getRegistry } from '../../../host/shellRegistry'
import { SidebarTabStrip } from './SidebarTabStrip'
import { useSidebarSplitStore } from './sidebarSplitStore'

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

function EmptyPlaceholder({ message }: { message: string }) {
  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '24px 16px',
        textAlign: 'center',
        color: 'var(--text-muted)',
        fontSize: 12,
        userSelect: 'none',
      }}
    >
      {message}
    </div>
  )
}

export function SidebarHost() {
  const entries = useSlotStore((s) => s.slots.sidebarContent)
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const activityItems = useActivityBarStore((s) => s.items)

  const leaves = useSidebarSplitStore((s) => s.leaves)
  const activeLeafId = useSidebarSplitStore((s) => s.activeLeafId)
  const setActiveLeaf = useSidebarSplitStore((s) => s.setActiveLeaf)
  const removeLeaf = useSidebarSplitStore((s) => s.removeLeaf)

  const workspaceName = rootPath ? basename(rootPath) : null

  // Title resolver — activity bar items map viewId → title. Falls back to
  // the raw type string when a leaf references a view with no activity-bar
  // entry (rare, but keeps the tab from rendering blank).
  const getTitle = useMemo(() => {
    const byViewId = new Map<string, string>()
    for (const item of activityItems) byViewId.set(item.viewId, item.title)
    return (type: string) => byViewId.get(type) ?? type
  }, [activityItems])

  const activeLeaf = leaves.find((l) => l.id === activeLeafId) ?? null
  const match = activeLeaf
    ? entries.find((e) => e.id === activeLeaf.type)
    : null

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
      {/* Tab strip — replaces the old 30px sidebar-top-row drag strip.
          Itself carries data-tauri-drag-region so empty space beyond
          the tabs drags the window. */}
      <SidebarTabStrip
        leaves={leaves}
        activeLeafId={activeLeafId}
        onSelect={setActiveLeaf}
        onClose={removeLeaf}
        getTitle={getTitle}
      />

      {/* Active leaf body — renders the view matching the active leaf's
          type. Surfaces an Obsidian-style placeholder when the registry
          has no match (plugin missing/disabled) or when the user has
          closed every tab. */}
      <div
        className="sidebar-leaf-body"
        style={{
          flex: 1,
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
          minHeight: 0,
        }}
      >
        {!activeLeaf ? (
          <EmptyPlaceholder message="No tabs open — pick one from the activity bar" />
        ) : !match ? (
          <EmptyPlaceholder message="No view — plugin missing or disabled" />
        ) : (
          createElement(match.component)
        )}
      </div>

      {/* Vault footer — unchanged. */}
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
