import { useWorkspaceStore } from './workspaceStore'

function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

export function WorkspaceStatusItem() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const open = useWorkspaceStore((s) => s.open)
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
        padding: '0 8px',
        cursor: 'pointer',
      }}
    >
      {rootPath ? basename(rootPath) : 'No workspace'}
    </button>
  )
}
