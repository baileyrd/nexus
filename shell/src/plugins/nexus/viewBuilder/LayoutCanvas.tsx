// BL-067 Phase 2c — visual layout canvas for the View Builder.
//
// Renders a scaled-down 2D preview of the workspace's macro layout
// (left | main | right with bottom spanning beneath). Two
// interactions:
//
//   • Drag a leaf chip → drop it on a different region → fires
//     `workspace.moveLeafToDock(leaf, side)`. This is the visual
//     equivalent of the inline "Move to" buttons that shipped in
//     Phase 2b — same mutator, same drop targets, but no menu open.
//
//   • Drag a divider line → calls `workspace.setSidedockSize(side, px)`
//     continuously. The user gets a live preview without stepping by
//     50px the way the Phase 2b -/+ buttons require.
//
// All geometry math lives in `canvasGeometry.ts` (pure module, unit
// tested) — the component only handles React state, pointer events,
// and rendering.

import { useEffect, useMemo, useReducer, useRef, useState } from 'react'

import { workspace } from '../../../workspace'
import {
  computeLayout,
  dividerAt,
  dragDividerToRealPx,
  extractCanvasState,
  regionAt,
  type CanvasLayout,
  type CanvasLeafChip,
  type DividerKind,
  type DockSide,
  type Rect,
} from './canvasGeometry'

const CANVAS_WIDTH = 360
const CANVAS_HEIGHT = 220

/** Drag state for an in-progress leaf drag. */
interface LeafDrag {
  kind: 'leaf'
  leafId: string
  /** Current pointer position in canvas-local coordinates. */
  x: number
  y: number
  /** Source region — used to skip the drop when the user releases on
   *  the same dock (a no-op call would still trigger a redundant
   *  `layout-change` event). */
  origin: DockSide
}

interface DividerDrag {
  kind: 'divider'
  divider: DividerKind
  /** Pointer position so we can keep the visual feedback line in sync. */
  x: number
  y: number
}

type DragState = LeafDrag | DividerDrag | null

function useLayoutVersion(): void {
  const [, force] = useReducer((x: number) => x + 1, 0)
  useEffect(() => {
    const off = workspace.on('layout-change', () => force())
    return off
  }, [])
}

export function LayoutCanvas(): React.ReactElement {
  // Re-render when the workspace mutates so the canvas reflects the
  // live state (matches `useLayoutVersion` in `WorkspaceRenderer`).
  useLayoutVersion()
  const [drag, setDrag] = useState<DragState>(null)
  const canvasRef = useRef<HTMLDivElement | null>(null)

  // Recompute on every render. `useLayoutVersion` above forces a
  // re-render on `layout-change`, which is exactly when the snapshot
  // shape can shift — so this cheap pure transform stays in sync
  // without subscribing to anything else.
  const state = extractCanvasState(workspace.layoutSnapshot(), workspace.activeLeafId)

  const layout = useMemo(
    () => computeLayout(
      { width: CANVAS_WIDTH, height: CANVAS_HEIGHT },
      state.sizes,
      state.collapsed,
    ),
    [state.sizes, state.collapsed],
  )

  // Hover hint while a drag is in progress: which side would the
  // current pointer position drop on? Used to highlight the target
  // region.
  const hoverSide: DockSide | null =
    drag?.kind === 'leaf' ? regionAt(layout, drag.x, drag.y) : null

  // Pointer-down on a leaf chip starts a leaf drag.
  const onLeafPointerDown = (
    e: React.PointerEvent<HTMLDivElement>,
    leafId: string,
    origin: DockSide,
  ) => {
    if (e.button !== 0) return
    const rect = canvasRef.current?.getBoundingClientRect()
    if (!rect) return
    e.currentTarget.setPointerCapture(e.pointerId)
    setDrag({
      kind: 'leaf',
      leafId,
      x: e.clientX - rect.left,
      y: e.clientY - rect.top,
      origin,
    })
    e.preventDefault()
  }

  // Pointer-down on the canvas background — only fires on dividers,
  // since regions stop propagation. The divider-zone hit test runs in
  // canvas-local coords.
  const onCanvasPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    const rect = canvasRef.current?.getBoundingClientRect()
    if (!rect) return
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top
    const div = dividerAt(layout, x, y)
    if (!div) return
    e.currentTarget.setPointerCapture(e.pointerId)
    setDrag({ kind: 'divider', divider: div, x, y })
    e.preventDefault()
  }

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!drag) return
    const rect = canvasRef.current?.getBoundingClientRect()
    if (!rect) return
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top
    if (drag.kind === 'divider') {
      const real = dragDividerToRealPx(
        drag.divider,
        { width: CANVAS_WIDTH, height: CANVAS_HEIGHT },
        x,
        y,
      )
      // Continuous resize — the workspace store clamps below 150px so
      // we don't bother re-clamping here.
      workspace.setSidedockSize(drag.divider, real)
      setDrag({ ...drag, x, y })
      return
    }
    setDrag({ ...drag, x, y })
  }

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!drag) return
    e.currentTarget.releasePointerCapture(e.pointerId)
    if (drag.kind === 'leaf') {
      const target = regionAt(layout, drag.x, drag.y)
      if (target && target !== drag.origin) {
        const leaf = workspace.leaves.get(drag.leafId)
        if (leaf) {
          workspace.moveLeafToDock(leaf, target)
        }
      }
    }
    setDrag(null)
  }

  const onPointerCancel = () => setDrag(null)

  return (
    <div
      ref={canvasRef}
      style={{
        position: 'relative',
        width: CANVAS_WIDTH,
        height: CANVAS_HEIGHT,
        background: 'var(--background-secondary)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 4,
        userSelect: 'none',
        cursor: drag?.kind === 'divider' ? dividerCursor(drag.divider) : 'default',
      }}
      onPointerDown={onCanvasPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      data-testid="view-builder-layout-canvas"
    >
      {(['left', 'main', 'right', 'bottom'] as const).map((side) => (
        <Region
          key={side}
          side={side}
          rect={layout[side]}
          leaves={state.regions[side]}
          collapsed={state.collapsed[side as 'left' | 'right' | 'bottom'] === true}
          highlight={hoverSide === side && drag?.kind === 'leaf' && drag.origin !== side}
          onLeafPointerDown={onLeafPointerDown}
          dragging={drag?.kind === 'leaf' ? drag.leafId : null}
        />
      ))}
      <DividerLines layout={layout} active={drag?.kind === 'divider' ? drag.divider : null} />
    </div>
  )
}

function dividerCursor(d: DividerKind): React.CSSProperties['cursor'] {
  return d === 'bottom' ? 'ns-resize' : 'ew-resize'
}

interface RegionProps {
  side: DockSide
  rect: Rect
  leaves: CanvasLeafChip[]
  collapsed: boolean
  highlight: boolean
  onLeafPointerDown: (
    e: React.PointerEvent<HTMLDivElement>,
    leafId: string,
    origin: DockSide,
  ) => void
  dragging: string | null
}

function Region({
  side,
  rect,
  leaves,
  collapsed,
  highlight,
  onLeafPointerDown,
  dragging,
}: RegionProps): React.ReactElement {
  return (
    <div
      data-region={side}
      style={{
        position: 'absolute',
        left: rect.x,
        top: rect.y,
        width: rect.w,
        height: rect.h,
        boxSizing: 'border-box',
        background: highlight
          ? 'var(--background-modifier-hover)'
          : side === 'main'
            ? 'var(--background-primary)'
            : 'var(--background-secondary)',
        outline: highlight ? '2px solid var(--interactive-accent, #3b82f6)' : 'none',
        outlineOffset: -2,
        padding: collapsed ? 0 : '4px 6px',
        overflow: 'hidden',
        fontSize: '0.75em',
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
      }}
    >
      {!collapsed && (
        <div
          style={{
            color: 'var(--text-muted)',
            fontSize: '0.85em',
            textTransform: 'uppercase',
            letterSpacing: '0.04em',
          }}
        >
          {side}
        </div>
      )}
      {!collapsed &&
        leaves.map((leaf) => (
          <div
            key={leaf.id}
            data-leaf-id={leaf.id}
            onPointerDown={(e) => onLeafPointerDown(e, leaf.id, side)}
            style={{
              padding: '2px 5px',
              background: leaf.active
                ? 'var(--interactive-accent, #3b82f6)'
                : 'var(--background-primary)',
              color: leaf.active ? 'var(--text-on-accent, #fff)' : 'var(--text-normal)',
              border: '1px solid var(--background-modifier-border)',
              borderRadius: 3,
              cursor: 'grab',
              opacity: dragging === leaf.id ? 0.4 : 1,
              fontSize: '0.85em',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
            title={`${leaf.type} — drag to move`}
          >
            {leaf.type}
          </div>
        ))}
    </div>
  )
}

function DividerLines({
  layout,
  active,
}: {
  layout: CanvasLayout
  active: DividerKind | null
}): React.ReactElement {
  const lineStyle = (highlight: boolean): React.CSSProperties => ({
    position: 'absolute',
    background: highlight ? 'var(--interactive-accent, #3b82f6)' : 'var(--background-modifier-border)',
    pointerEvents: 'none',
  })
  return (
    <>
      <div
        data-divider="left"
        style={{
          ...lineStyle(active === 'left'),
          left: layout.left.w - 1,
          top: 0,
          width: 2,
          height: layout.main.h,
        }}
      />
      <div
        data-divider="right"
        style={{
          ...lineStyle(active === 'right'),
          left: layout.right.x - 1,
          top: 0,
          width: 2,
          height: layout.main.h,
        }}
      />
      <div
        data-divider="bottom"
        style={{
          ...lineStyle(active === 'bottom'),
          left: 0,
          top: layout.bottom.y - 1,
          width: layout.left.w + layout.main.w + layout.right.w,
          height: 2,
        }}
      />
    </>
  )
}
