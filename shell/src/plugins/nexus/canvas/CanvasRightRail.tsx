// Vertical chrome rail anchored to the top-right of the canvas viewport.
// Mirrors Obsidian's canvas toolbar — settings popup, zoom controls,
// undo/redo, help — and dispatches to the active canvas handle so the
// existing keyboard pathway and the chrome buttons share one code path.
//
// Settings popup contents:
//   - Snap to grid (real, wired to the existing grid toggle)
//   - Snap to objects (stub — Coming soon)
//   - Read-only       (stub — Coming soon)
//
// All buttons are inert when there is no active canvas handle (never
// happens in practice since the rail only mounts inside CanvasView).

import { useEffect, useRef, useState, type CSSProperties } from 'react'
import type { CanvasHandle } from './activeCanvas'

interface Props {
  handle: CanvasHandle
  showGrid: boolean
  /** Coming-soon toast factory — `() => void`. */
  comingSoon: (label: string) => () => void
}

export function CanvasRightRail({ handle, showGrid, comingSoon }: Props) {
  const [settingsOpen, setSettingsOpen] = useState(false)
  const settingsRef = useRef<HTMLDivElement | null>(null)

  // Click-outside dismiss for the settings popup.
  useEffect(() => {
    if (!settingsOpen) return
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node | null
      if (!t) return
      if (settingsRef.current?.contains(t)) return
      setSettingsOpen(false)
    }
    document.addEventListener('mousedown', onDown, true)
    return () => document.removeEventListener('mousedown', onDown, true)
  }, [settingsOpen])

  return (
    <div
      data-canvas-export-exclude="true"
      style={{
        position: 'absolute',
        top: 12,
        right: 12,
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        padding: 4,
        borderRadius: 6,
        background: 'var(--background-secondary)',
        border: '1px solid var(--divider-color)',
        boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
        pointerEvents: 'auto',
      }}
    >
      <div ref={settingsRef} style={{ position: 'relative' }}>
        <RailButton
          title="Canvas settings"
          onClick={() => setSettingsOpen((v) => !v)}
          active={settingsOpen}
        >
          ⚙
        </RailButton>
        {settingsOpen && (
          <div
            style={{
              position: 'absolute',
              top: 0,
              right: '110%',
              minWidth: 180,
              padding: 4,
              borderRadius: 6,
              background: 'var(--background-secondary)',
              border: '1px solid var(--divider-color)',
              boxShadow: '0 4px 12px rgba(0,0,0,0.4)',
            }}
          >
            <PopupRow
              label="Snap to grid"
              checked={showGrid}
              onClick={() => handle.toggleGrid()}
            />
            <PopupRow
              label="Snap to objects"
              checked={true}
              onClick={comingSoon('Snap to objects')}
            />
            <PopupRow
              label="Read-only"
              checked={false}
              onClick={comingSoon('Read-only canvas')}
            />
          </div>
        )}
      </div>
      <RailButton title="Zoom in" onClick={() => handle.zoomBy(1.2)}>
        +
      </RailButton>
      <RailButton title="Reset zoom" onClick={() => handle.resetZoom()}>
        ↻
      </RailButton>
      <RailButton title="Zoom to fit (Shift+1)" onClick={() => handle.fit()}>
        ⛶
      </RailButton>
      <RailButton title="Zoom out" onClick={() => handle.zoomBy(1 / 1.2)}>
        −
      </RailButton>
      <RailButton title="Undo" onClick={() => handle.undo()}>
        ↶
      </RailButton>
      <RailButton title="Redo" onClick={() => handle.redo()}>
        ↷
      </RailButton>
      <RailButton title="Canvas help" onClick={() => handle.toggleHelp()}>
        ?
      </RailButton>
    </div>
  )
}

const RAIL_BUTTON_BASE: CSSProperties = {
  width: 28,
  height: 28,
  display: 'inline-grid',
  placeItems: 'center',
  border: '1px solid transparent',
  borderRadius: 4,
  background: 'transparent',
  color: 'var(--text-normal)',
  cursor: 'pointer',
  fontSize: 14,
  lineHeight: 1,
  padding: 0,
}

function RailButton({
  title,
  onClick,
  active,
  children,
}: {
  title: string
  onClick: () => void
  active?: boolean
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      style={{
        ...RAIL_BUTTON_BASE,
        background: active ? 'var(--background-modifier-hover)' : 'transparent',
        borderColor: active ? 'var(--divider-color)' : 'transparent',
      }}
    >
      {children}
    </button>
  )
}

function PopupRow({
  label,
  checked,
  onClick,
}: {
  label: string
  checked: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        width: '100%',
        padding: '6px 10px',
        background: 'transparent',
        border: 'none',
        color: 'var(--text-normal)',
        cursor: 'pointer',
        fontSize: 12,
        textAlign: 'left',
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = 'var(--background-modifier-hover)'
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = 'transparent'
      }}
    >
      <span style={{ flex: 1 }}>{label}</span>
      <span
        aria-hidden
        style={{
          color: checked ? 'var(--interactive-accent)' : 'transparent',
          fontSize: 13,
        }}
      >
        ✓
      </span>
    </button>
  )
}
