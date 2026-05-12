import { useWorkspaceStore } from '../workspace/workspaceStore'

export function WorkspaceStatus() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const synced = rootPath !== null
  const label = synced ? `Forge synced` : 'No workspace'

  return (
    <span
      title={rootPath ?? undefined}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        padding: '0 2px',
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
      <span>{label}</span>
    </span>
  )
}
