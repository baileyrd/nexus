import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useWorkspaceStore } from '../workspace/workspaceStore'

function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

const baseButtonStyle: React.CSSProperties = {
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
    ...baseButtonStyle,
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

export function TitleBar() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const openWorkspace = useWorkspaceStore((s) => s.open)
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
      }}
    >
      <button
        type="button"
        onClick={() => openWorkspace()}
        title={rootPath ?? 'No workspace open — click to choose a folder'}
        style={{
          background: 'transparent',
          border: 'none',
          color: 'inherit',
          font: 'inherit',
          padding: '0 12px',
          cursor: 'pointer',
          height: '100%',
        }}
      >
        {rootPath ? basename(rootPath) : 'No workspace'}
      </button>

      {/* Drag region: middle spacer only. Keeping interactive buttons out of
          the drag region avoids eaten pointer events on Windows/Tauri 2. */}
      <div data-tauri-drag-region style={{ flex: 1, height: '100%' }} />

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
