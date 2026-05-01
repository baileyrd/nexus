import { useWorkspaceStore } from './workspaceStore'

export function WorkspaceStatusItem() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const open = useWorkspaceStore((s) => s.open)
  const synced = rootPath !== null

  return (
    <button
      type="button"
      onClick={() => open()}
      title={rootPath ?? 'No workspace open — click to choose a folder'}
      style={{
        background: 'transparent',
        border: 'none',
        color: 'inherit',
        font: 'inherit',
        padding: '0 4px',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
      }}
    >
      <span
        aria-hidden
        style={{
          width: 6,
          height: 6,
          borderRadius: '50%',
          flexShrink: 0,
          background: synced ? 'var(--ok)' : 'var(--text-faint)',
          boxShadow: synced ? '0 0 4px var(--ok)' : 'none',
        }}
      />
      <span>{synced ? 'Forge synced' : 'No workspace'}</span>
    </button>
  )
}
