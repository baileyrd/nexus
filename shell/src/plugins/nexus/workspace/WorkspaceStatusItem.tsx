import { useWorkspaceStore } from './workspaceStore'

function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

export function WorkspaceStatusItem() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const open = useWorkspaceStore((s) => s.open)
  const synced = rootPath !== null
  const label = synced ? basename(rootPath!) : 'No workspace'

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
          background: synced ? 'var(--interactive-accent)' : 'var(--text-faint)',
          boxShadow: synced ? '0 0 6px var(--interactive-accent)' : 'none',
        }}
      />
      <span>{label}</span>
    </button>
  )
}
