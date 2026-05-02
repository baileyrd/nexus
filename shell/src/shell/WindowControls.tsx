// Window min/max/close cluster. Lives inside a column's top-row; no
// window-level chrome. Extracted from the deleted `nexus.titleBar`
// plugin during the Obsidian-faithful column refactor (Task 6).
//
// SH-015: Platform-branched. On macOS the cluster renders as traffic-light
// circles (close/min/max, top-left). On Windows/Linux it renders as the
// Win11-style elongated buttons (min/max/close, top-right). The layout
// branch is driven by `body.mod-macos` which `installBodyClasses()` sets
// synchronously before React mounts, so it is stable for the lifetime of
// the app and safe to read during render without state.
//
// The surrounding drag region is provided by the parent row via
// `data-tauri-drag-region`; these buttons are plain <button>s so click
// events fire normally (Tauri 2 on Windows swallows clicks inside a
// drag region).
import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { clientLogger } from '../host/clientLogger'

// Resolved once at module init — `installBodyClasses()` is guaranteed to
// run before any plugin or React tree is mounted.
export const IS_MACOS =
  typeof document !== 'undefined' &&
  document.body.classList.contains('mod-macos')

// ─── Win/Linux style ────────────────────────────────────────────────────────

const baseWinStyle: React.CSSProperties = {
  // SH-004: height tracks --chrome-row-height so Win/Linux controls scale
  // with density. Width is slightly wider (square-ish) for hit-target size.
  width: 'var(--chrome-row-height)',
  height: 'var(--chrome-row-height)',
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

function WinButton({
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
  // Windows convention: close button hovers to red; others use a subtle
  // raised background from the token palette.
  const hoverBg = closeAccent ? '#e81123' : 'var(--background-modifier-hover)'
  const hoverFg = closeAccent ? '#ffffff' : 'var(--text-normal)'
  const style: React.CSSProperties = {
    ...baseWinStyle,
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

// ─── macOS traffic-light style ───────────────────────────────────────────────

// Fixed macOS traffic-light colors. Not token-driven intentionally — these
// must be literal system colors to feel native regardless of theme.
const MAC_CLOSE   = '#ff5f57'
const MAC_MIN     = '#febc2e'
const MAC_MAX     = '#28c840'
const MAC_HOVER_SHADOW = 'rgba(0,0,0,0.3)'

function MacButton({
  onClick,
  label,
  color,
  symbol,
}: {
  onClick: () => void
  label: string
  color: string
  symbol: string
}) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      aria-label={label}
      title={label}
      style={{
        width: 12,
        height: 12,
        borderRadius: '50%',
        background: color,
        border: 'none',
        cursor: 'pointer',
        padding: 0,
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        boxShadow: hover ? `inset 0 0 0 1px ${MAC_HOVER_SHADOW}` : 'none',
        transition: 'box-shadow 0.08s',
        fontSize: 8,
        lineHeight: 1,
        color: hover ? 'rgba(0,0,0,0.7)' : 'transparent',
        userSelect: 'none',
      }}
    >
      {hover ? symbol : null}
    </button>
  )
}

// ─── Shared icons ────────────────────────────────────────────────────────────

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

// ─── Exported component ──────────────────────────────────────────────────────

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
        clientLogger.warn('[WindowControls] failed to wire maximize listener:', err)
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

  if (IS_MACOS) {
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '0 8px',
          height: 'var(--chrome-row-height)',
          flexShrink: 0,
        }}
      >
        {/* macOS order: close · minimize · maximize */}
        <MacButton onClick={close}          label="Close"    color={MAC_CLOSE} symbol="✕" />
        <MacButton onClick={minimize}       label="Minimize" color={MAC_MIN}   symbol="−" />
        <MacButton onClick={toggleMaximize} label={maximized ? 'Restore' : 'Maximize'} color={MAC_MAX} symbol={maximized ? '⤡' : '+'} />
      </div>
    )
  }

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
        background: 'var(--tab-container-background)',
      }}
    >
      <WinButton onClick={minimize} label="Minimize">
        <MinimizeIcon />
      </WinButton>
      <WinButton
        onClick={toggleMaximize}
        label={maximized ? 'Restore' : 'Maximize'}
      >
        {maximized ? <RestoreIcon /> : <MaximizeIcon />}
      </WinButton>
      <WinButton onClick={close} label="Close" closeAccent>
        <CloseIcon />
      </WinButton>
    </div>
  )
}
