import { useWorkspaceStore } from './workspaceStore'
import { useConnectionState, type ConnectionState } from './useConnectionState'

function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

/** BL-140 Phase 3b — match the backend's URI detection so the badge
 *  can branch on remote vs local without consulting the bridge. */
function isRemoteUri(root: string): boolean {
  return root.includes('://')
}

/** Render a remote `ssh://user@host[:port]/path` as a compact
 *  `host:basename` label. Falls back to the raw URI if the host part
 *  can't be extracted. */
function remoteLabel(uri: string): string {
  try {
    // Strip scheme.
    const afterScheme = uri.replace(/^[a-z]+:\/\//, '')
    // Strip optional `user@`.
    const auth = afterScheme.includes('/')
      ? afterScheme.slice(0, afterScheme.indexOf('/'))
      : afterScheme
    const path = afterScheme.slice(afterScheme.indexOf('/'))
    const hostWithPort = auth.includes('@') ? auth.split('@')[1] : auth
    const host = hostWithPort.startsWith('[')
      ? hostWithPort.slice(0, hostWithPort.indexOf(']') + 1)
      : hostWithPort.split(':')[0]
    const tail = basename(path)
    return `${host}:${tail}`
  } catch {
    return uri
  }
}

/** Connection-state palette. Local forges always get the accent
 *  (their kernel is in-process; "connected" is the only meaningful
 *  state). Remote forges discriminate. */
function dotColor(state: ConnectionState, remote: boolean, hasRoot: boolean): string {
  if (!hasRoot) return 'var(--text-faint)'
  if (!remote) return 'var(--interactive-accent)'
  switch (state) {
    case 'connected':
      return 'var(--interactive-accent)'
    case 'reconnecting':
      // Yellow-ish — using accent + warning if available, fall back to
      // a literal color. Most themes expose `--text-warning`.
      return 'var(--text-warning, #d4b942)'
    case 'disconnected':
      return 'var(--text-error, #d44a4a)'
    case 'idle':
    default:
      // Still pre-first-call. Render same as local "open" so the badge
      // doesn't flash a warning state on a healthy boot.
      return 'var(--interactive-accent)'
  }
}

function dotShadow(state: ConnectionState, remote: boolean, hasRoot: boolean): string {
  if (!hasRoot) return 'none'
  if (!remote) return '0 0 6px var(--interactive-accent)'
  switch (state) {
    case 'reconnecting':
      return '0 0 6px var(--text-warning, #d4b942)'
    case 'disconnected':
      return '0 0 6px var(--text-error, #d44a4a)'
    default:
      return '0 0 6px var(--interactive-accent)'
  }
}

/** Tooltip text — for remote forges, surfaces the connection state +
 *  full URI so the user understands what the badge is conveying. */
function tooltip(
  rootPath: string | null,
  remote: boolean,
  state: ConnectionState,
): string {
  if (!rootPath) return 'No workspace open — click to choose a folder'
  if (!remote) return rootPath
  const stateLabel =
    state === 'reconnecting'
      ? 'reconnecting…'
      : state === 'disconnected'
      ? 'disconnected (next call will retry)'
      : state === 'idle'
      ? 'idle (not yet dispatched)'
      : 'connected'
  return `${rootPath} — ${stateLabel}`
}

export function WorkspaceStatusItem() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const open = useWorkspaceStore((s) => s.open)
  const connectionState = useConnectionState()
  const hasRoot = rootPath !== null
  const remote = hasRoot && isRemoteUri(rootPath!)
  const label = hasRoot
    ? remote
      ? remoteLabel(rootPath!)
      : basename(rootPath!)
    : 'No workspace'

  return (
    <button
      type="button"
      onClick={() => open()}
      title={tooltip(rootPath, remote, connectionState)}
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
          background: dotColor(connectionState, remote, hasRoot),
          boxShadow: dotShadow(connectionState, remote, hasRoot),
        }}
      />
      <span>{label}</span>
      {remote && connectionState === 'reconnecting' && (
        <span style={{ fontSize: 11, opacity: 0.7 }}> · reconnecting</span>
      )}
      {remote && connectionState === 'disconnected' && (
        <span style={{ fontSize: 11, opacity: 0.7 }}> · offline</span>
      )}
    </button>
  )
}
