import { useWorkspaceStore } from '../workspace/workspaceStore'

// BL-053 Phase 1 — forge-name + ember dot. The dot uses the same
// `--interactive-accent` ember token the rest of the chrome
// already adopts so the bottom-right corner matches the mockup's
// "lap-working · •" pattern. Disconnected workspaces get a muted
// dot without the glow.
export function WorkspaceStatus() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const synced = rootPath !== null
  const forgeName = synced ? deriveForgeName(rootPath) : 'No forge'
  const dotColor = synced ? 'var(--interactive-accent)' : 'var(--text-faint)'

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
          background: dotColor,
          boxShadow: synced ? '0 0 4px var(--interactive-accent)' : 'none',
        }}
      />
      <span>{forgeName}</span>
    </span>
  )
}

/**
 * Derive a compact display name from the forge's absolute path.
 * Matches the mockup's `lap-working` short-label style: the
 * directory name, lowercased, with separators collapsed to `-`.
 * `/Users/dave/Projects/Lap Working/` → `lap-working`.
 */
function deriveForgeName(absPath: string): string {
  const trimmed = absPath.replace(/[/\\]+$/, '')
  const tail = trimmed.split(/[/\\]/).pop() ?? trimmed
  return tail.trim().toLowerCase().replace(/\s+/g, '-') || 'forge'
}
