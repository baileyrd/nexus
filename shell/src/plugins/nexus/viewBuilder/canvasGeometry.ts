// BL-067 Phase 2c — pure geometry helpers for the visual layout canvas.
//
// In addition to drag/hit-test math this module exports
// `extractCanvasState` — a pure transform from a `WorkspaceJSON`
// snapshot into the {sizes, collapsed, regions} triple the canvas
// renders against. Parametric inputs make it unit-testable without
// rendering React or seeding the live workspace store.
//
// The canvas mirrors the workspace's macro layout (left dock | main |
// right dock, with bottom dock spanning beneath) at a small scale.
// All drag/hit-test math lives here so it can be unit-tested without
// rendering React.
//
// Coordinate system: canvas-local pixels, origin at the canvas's
// top-left. The functions never read `window` or DOM state — callers
// pass pointer coordinates already translated into canvas space.

/** Pixel size of the visual canvas inside the View Builder panel. */
export interface CanvasSize {
  width: number
  height: number
}

/** Live dock sizes pulled from the workspace store. Pixel values
 *  measured against the real viewport — the canvas scales them down. */
export interface DockSizes {
  left: number
  right: number
  bottom: number
}

/** Per-dock collapsed flags. A collapsed dock is rendered as a thin
 *  spine at canvas edge (still visible so the user can drag a leaf
 *  back into it; the visible width is `COLLAPSED_SPINE`). */
export interface CollapsedFlags {
  left: boolean
  right: boolean
  bottom: boolean
}

/** Side identifiers used by the workspace's mutator API. The canvas
 *  hit-tester returns one of these so the caller can route into
 *  `moveLeafToDock(leaf, side)` directly. */
export type DockSide = 'left' | 'right' | 'main' | 'bottom'

/** Visible width of a collapsed dock in canvas-local pixels. Big
 *  enough to remain a drop target, small enough to read as
 *  "collapsed". */
export const COLLAPSED_SPINE = 8

/** Width of a divider hit zone (centered on the rendered line). */
export const DIVIDER_HIT_PX = 6

/** Maximum fraction of the canvas a single side dock can consume.
 *  Caps the visual at 35% so even a heavily-resized real dock leaves
 *  room for the main pane to remain meaningful. */
export const MAX_DOCK_FRACTION = 0.35

/** Reference workspace width in real pixels. Dock sizes get scaled by
 *  `canvas.width / TYPICAL_WORKSPACE_WIDTH` so the canvas reflects
 *  roughly the same proportions the user sees in the live shell. */
export const TYPICAL_WORKSPACE_WIDTH = 1200
export const TYPICAL_WORKSPACE_HEIGHT = 800

/** Scale a real-px dock size into canvas-px, clamped to a sane visual
 *  range so a tiny dock isn't invisible and a giant dock doesn't eat
 *  the canvas. */
export function scaleDockWidth(
  realPx: number,
  canvasWidth: number,
  collapsed: boolean,
): number {
  if (collapsed) return COLLAPSED_SPINE
  const scaled = realPx * (canvasWidth / TYPICAL_WORKSPACE_WIDTH)
  const min = COLLAPSED_SPINE * 2
  const max = canvasWidth * MAX_DOCK_FRACTION
  return Math.max(min, Math.min(max, scaled))
}

export function scaleDockHeight(
  realPx: number,
  canvasHeight: number,
  collapsed: boolean,
): number {
  if (collapsed) return COLLAPSED_SPINE
  const scaled = realPx * (canvasHeight / TYPICAL_WORKSPACE_HEIGHT)
  const min = COLLAPSED_SPINE * 2
  const max = canvasHeight * MAX_DOCK_FRACTION
  return Math.max(min, Math.min(max, scaled))
}

/** Inverse of `scaleDockWidth`: convert a canvas-px width back into a
 *  real-px dock size. Used during a divider drag — the user moves the
 *  divider in canvas space, the workspace store needs the value in
 *  real pixels. Snaps to the integer real-px so consecutive drag
 *  ticks settle to identical workspace state. */
export function canvasWidthToRealPx(canvasPx: number, canvasWidth: number): number {
  return Math.round(canvasPx / (canvasWidth / TYPICAL_WORKSPACE_WIDTH))
}

export function canvasHeightToRealPx(canvasPx: number, canvasHeight: number): number {
  return Math.round(canvasPx / (canvasHeight / TYPICAL_WORKSPACE_HEIGHT))
}

/** Rectangle in canvas-local coordinates. */
export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

/** All four region rects for the current canvas + dock state. The
 *  bottom dock spans the full canvas width (matching the live shell's
 *  layout — bottom is below side docks). The render layer keys off
 *  these directly. */
export interface CanvasLayout {
  left: Rect
  main: Rect
  right: Rect
  bottom: Rect
}

/** Compute region rects given the canvas size, dock sizes, and
 *  collapsed flags. The math is allocation: fixed canvas → bottom
 *  dock claims its height first, the remaining vertical strip is then
 *  split into left | main | right horizontally. */
export function computeLayout(
  canvas: CanvasSize,
  sizes: DockSizes,
  collapsed: CollapsedFlags,
): CanvasLayout {
  const bottomH = scaleDockHeight(sizes.bottom, canvas.height, collapsed.bottom)
  const topH = Math.max(0, canvas.height - bottomH)
  const leftW = scaleDockWidth(sizes.left, canvas.width, collapsed.left)
  const rightW = scaleDockWidth(sizes.right, canvas.width, collapsed.right)
  const mainW = Math.max(0, canvas.width - leftW - rightW)
  return {
    left: { x: 0, y: 0, w: leftW, h: topH },
    main: { x: leftW, y: 0, w: mainW, h: topH },
    right: { x: leftW + mainW, y: 0, w: rightW, h: topH },
    bottom: { x: 0, y: topH, w: canvas.width, h: bottomH },
  }
}

/** Hit-test: which region (if any) does the canvas-local pointer fall
 *  in? Returns `null` only when the pointer is outside the canvas
 *  bounds — every interior point falls into exactly one region. */
export function regionAt(layout: CanvasLayout, x: number, y: number): DockSide | null {
  const sides: DockSide[] = ['left', 'main', 'right', 'bottom']
  for (const side of sides) {
    const r = layout[side]
    if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return side
  }
  return null
}

/** Identifier for a draggable divider between regions. */
export type DividerKind = 'left' | 'right' | 'bottom'

/** Hit-test a divider — pointer must be within `DIVIDER_HIT_PX` of the
 *  divider line and within the divider's perpendicular extent.
 *  Returns the matching `DividerKind` or `null`. Dividers are checked
 *  in order: left and right are vertical lines spanning the top
 *  region (left/main border, main/right border); bottom is a
 *  horizontal line across the canvas (between top and bottom). */
export function dividerAt(layout: CanvasLayout, x: number, y: number): DividerKind | null {
  // left divider: vertical, at x = layout.left.w, spans top region
  if (
    Math.abs(x - layout.left.w) <= DIVIDER_HIT_PX / 2 &&
    y >= 0 &&
    y < layout.main.h
  ) {
    return 'left'
  }
  // right divider: vertical, at x = layout.right.x, spans top region
  if (
    Math.abs(x - layout.right.x) <= DIVIDER_HIT_PX / 2 &&
    y >= 0 &&
    y < layout.main.h
  ) {
    return 'right'
  }
  // bottom divider: horizontal, at y = layout.bottom.y, spans full width
  if (
    Math.abs(y - layout.bottom.y) <= DIVIDER_HIT_PX / 2 &&
    x >= 0 &&
    x < layout.bottom.x + layout.bottom.w
  ) {
    return 'bottom'
  }
  return null
}

/** Per-side leaf-chip data — what the canvas renders inside each
 *  region. */
export interface CanvasLeafChip {
  id: string
  type: string
  active: boolean
}

/** Snapshot-derived state the canvas component renders against.
 *  Pure-data — no React, no workspace store. */
export interface CanvasState {
  sizes: { left: number; right: number; bottom: number }
  collapsed: { left: boolean; right: boolean; bottom: boolean }
  regions: Record<DockSide, CanvasLeafChip[]>
}

/** Minimal subset of `SerializedNode` the extractor walks. Kept as a
 *  structural type so callers don't have to reach into the shell's
 *  full workspace types module from the test file. */
export interface MinimalSerializedNode {
  kind: string
  side?: 'left' | 'right' | 'bottom'
  collapsed?: boolean
  size?: number
  children?: MinimalSerializedNode[]
  leaves?: Array<{ id: string; viewState: { type: string } }>
  child?: MinimalSerializedNode
}

/** Extract the dock sizes, collapsed flags, and per-region leaf chips
 *  from a workspace snapshot. Pure transform. The active-leaf id is
 *  carried through so the renderer can highlight the active chip
 *  consistently with the live shell. */
export function extractCanvasState(
  snapshot: {
    main?: MinimalSerializedNode
    left?: MinimalSerializedNode
    right?: MinimalSerializedNode
    bottom?: MinimalSerializedNode
  },
  activeLeafId: string | null,
): CanvasState {
  const dockMeta = (node: MinimalSerializedNode | undefined) => {
    if (!node) return { size: 0, collapsed: true }
    if (node.kind !== 'split') return { size: 0, collapsed: false }
    return { size: node.size ?? 0, collapsed: node.collapsed === true }
  }
  const left = dockMeta(snapshot.left)
  const right = dockMeta(snapshot.right)
  const bottom = dockMeta(snapshot.bottom)
  return {
    sizes: { left: left.size, right: right.size, bottom: bottom.size },
    collapsed: { left: left.collapsed, right: right.collapsed, bottom: bottom.collapsed },
    regions: {
      left: collectChips(snapshot.left, activeLeafId),
      main: collectChips(snapshot.main, activeLeafId),
      right: collectChips(snapshot.right, activeLeafId),
      bottom: collectChips(snapshot.bottom, activeLeafId),
    },
  }
}

function collectChips(
  node: MinimalSerializedNode | undefined,
  activeId: string | null,
): CanvasLeafChip[] {
  if (!node) return []
  const out: CanvasLeafChip[] = []
  walkChips(node, activeId, out)
  return out
}

function walkChips(
  node: MinimalSerializedNode,
  activeId: string | null,
  out: CanvasLeafChip[],
): void {
  if (node.kind === 'split' && node.children) {
    for (const c of node.children) walkChips(c, activeId, out)
    return
  }
  if (node.kind === 'tabs' && node.leaves) {
    for (const l of node.leaves) {
      out.push({ id: l.id, type: l.viewState.type, active: l.id === activeId })
    }
    return
  }
  if ((node.kind === 'root' || node.kind === 'floating') && node.child) {
    walkChips(node.child, activeId, out)
    return
  }
  if (node.kind === 'leaf') {
    // Defensive: the workspace's snapshot wraps loose leaves in a Tabs
    // (every dock has at least one Tabs root), but parse-tolerantly
    // handle bare leaves so a hand-authored test fixture still works.
    const bare = node as MinimalSerializedNode & {
      id?: string
      viewState?: { type: string }
    }
    if (bare.id && bare.viewState) {
      out.push({ id: bare.id, type: bare.viewState.type, active: bare.id === activeId })
    }
  }
}

/** Compute the new real-px dock size for a divider-drag in progress.
 *  - For `left`: pointer x grows the left dock. New canvas-px width
 *    is the pointer's x.
 *  - For `right`: pointer x shrinks the right dock from the left edge.
 *    New canvas-px width is `canvas.width - x`.
 *  - For `bottom`: pointer y shrinks the bottom dock from the top.
 *    New canvas-px height is `canvas.height - y`.
 *
 *  Returns the value in real-px so the caller can pass it directly to
 *  `setSidedockSize`. The workspace store applies its own 150-real-px
 *  floor — this helper just does the geometry. */
export function dragDividerToRealPx(
  divider: DividerKind,
  canvas: CanvasSize,
  pointerX: number,
  pointerY: number,
): number {
  if (divider === 'left') {
    return canvasWidthToRealPx(Math.max(0, pointerX), canvas.width)
  }
  if (divider === 'right') {
    return canvasWidthToRealPx(Math.max(0, canvas.width - pointerX), canvas.width)
  }
  // bottom
  return canvasHeightToRealPx(Math.max(0, canvas.height - pointerY), canvas.height)
}
