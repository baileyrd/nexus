// Window min/max/close cluster. Lives inside a column's top-row; no
// window-level chrome. Extracted from the deleted `nexus.titleBar`
// plugin during the Obsidian-faithful column refactor (Task 6).
//
// The surrounding drag region is provided by the parent row via
// `data-tauri-drag-region`; these buttons are plain <button>s so click
// events fire normally (Tauri 2 on Windows swallows clicks inside a
// drag region).
import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

const baseControlStyle: React.CSSProperties = {
  width: 40,
  height: 36,
  background: 'transparent',
  border: 'none',
  color: 'var(--text-muted)',
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
  const hoverBg = closeAccent ? '#e81123' : 'var(--background-modifier-hover)'
  const hoverFg = closeAccent ? '#ffffff' : 'var(--text-normal)'
  const style: React.CSSProperties = {
    ...baseControlStyle,
    background: hover ? hoverBg : 'transparent',
    color: hover ? hoverFg : 'var(--text-muted)',
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

export function WindowControls() {
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
        console.warn('[WindowControls] failed to wire maximize listener:', err)
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
        flexShrink: 0,
        display: 'flex',
        alignItems: 'stretch',
        zIndex: 1,
        // Match the tab-strip background underneath so the absolutely-
        // positioned cluster reads as continuous chrome with the trailing
        // tab controls (chevron, right-sidedock toggle) sitting just to
        // its left, instead of a floating cluster on top of the strip.
        background: 'var(--tab-container-background, var(--background-secondary-alt, #2d2d2d))',
      }}
    >
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
