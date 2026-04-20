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
      className="clickable-icon"
      style={{
        width: 22,
        height: 22,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--text-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        borderRadius: 'var(--radius-s)',
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

  // Title + icon resolver — activity bar items map viewId → title/iconName.
  const getMeta = useMemo(() => {
    const byViewId = new Map<string, { title: string; iconName?: string }>()
    for (const item of activityItems)
      byViewId.set(item.viewId, { title: item.title, iconName: item.iconName })
    return (type: string) => byViewId.get(type) ?? { title: type }
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
      className="workspace-tabs mod-top"
      style={{
        height: '100%',
        width: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {/* Sidebar tab strip — `.workspace-tab-header-container` with
          inner `.workspace-tab-header-container-inner` scroll element. */}
      <SidebarTabStrip
        leaves={leaves}
        activeLeafId={activeLeafId}
        onSelect={setActiveLeaf}
        onClose={removeLeaf}
        getMeta={getMeta}
      />

      {/* Active leaf body. */}
      <div
        className="workspace-leaf"
        style={{
          flex: 1,
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
          minHeight: 0,
        }}
      >
        <div
          className="workspace-leaf-content"
          style={{
            flex: 1,
            minHeight: 0,
            display: 'flex',
            flexDirection: 'column',
            overflow: 'hidden',
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
      </div>

      {/* Vault footer — analogous to Obsidian's .workspace-sidedock-vault-profile. */}
      <div
        className="workspace-sidedock-vault-profile"
        style={{
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '0 6px 0 10px',
          height: 32,
          borderTop: 'var(--divider-width) solid var(--divider-color)',
          fontSize: 12,
          color: 'var(--text-muted)',
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
