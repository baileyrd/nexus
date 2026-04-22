// 2D-canvas renderer for .canvas documents. Kept imperative + framework-
// free so the RAF hot path doesn't allocate closures or React fibers
// every frame. Called from CanvasView's requestAnimationFrame loop.

import type { CanvasDoc, CanvasEdge, CanvasNode } from './kernelClient'
import type { Camera } from './canvasStore'

interface Theme {
  bgMuted: string
  bgRaised: string
  fg: string
  fgMuted: string
  accent: string
  border: string
}

/** Fallback theme tokens — the shell's CSS vars aren't queryable from
 *  <canvas>, so we read them via getComputedStyle on the canvas element
 *  and cache the result. Values here match the dark-theme defaults in
 *  the existing shell. */
const FALLBACK_THEME: Theme = {
  bgMuted: '#1e1e1e',
  bgRaised: '#2d2d2d',
  fg: '#e5e7eb',
  fgMuted: '#9ca3af',
  accent: '#3b82f6',
  border: '#3f3f46',
}

/** Default rectangle for freshly-created text nodes — matches Obsidian's
 *  "double-click empty space" behaviour. */
export const DEFAULT_TEXT_NODE_SIZE = { width: 250, height: 60 }

/** Minimum node dimensions in world units, enforced on resize so a
 *  careless drag can't collapse a node into an invisible sliver. */
export const MIN_NODE_SIZE = 40

/** Half-size of a resize handle in CSS pixels. Handles stay this many
 *  screen pixels wide at every zoom level so they're always clickable. */
export const HANDLE_HALF_CSS = 5

export type ResizeHandle =
  | 'nw' | 'n' | 'ne'
  | 'w' |       'e'
  | 'sw' | 's' | 'se'

export const RESIZE_HANDLES: readonly ResizeHandle[] = [
  'nw', 'n', 'ne', 'w', 'e', 'sw', 's', 'se',
] as const

/** Return the centre of `handle` on `node` in world coordinates. */
export function handleCentre(node: CanvasNode, handle: ResizeHandle): { x: number; y: number } {
  const cx = node.x + node.width / 2
  const cy = node.y + node.height / 2
  const x2 = node.x + node.width
  const y2 = node.y + node.height
  switch (handle) {
    case 'nw': return { x: node.x, y: node.y }
    case 'n':  return { x: cx,     y: node.y }
    case 'ne': return { x: x2,     y: node.y }
    case 'w':  return { x: node.x, y: cy }
    case 'e':  return { x: x2,     y: cy }
    case 'sw': return { x: node.x, y: y2 }
    case 's':  return { x: cx,     y: y2 }
    case 'se': return { x: x2,     y: y2 }
  }
}

/** Hit-test the eight resize handles of `node` at screen position
 *  `(screenX, screenY)` (CSS pixels inside the canvas). Returns the
 *  first handle whose bounding square contains the point, or `null`. */
export function hitTestHandle(
  node: CanvasNode,
  camera: Camera,
  screenX: number,
  screenY: number,
): ResizeHandle | null {
  for (const h of RESIZE_HANDLES) {
    const c = handleCentre(node, h)
    const sx = (c.x - camera.x) * camera.zoom
    const sy = (c.y - camera.y) * camera.zoom
    if (
      Math.abs(screenX - sx) <= HANDLE_HALF_CSS + 2 &&
      Math.abs(screenY - sy) <= HANDLE_HALF_CSS + 2
    ) {
      return h
    }
  }
  return null
}

/** CSS cursor name appropriate for pointing at `handle`. */
export function cursorForHandle(handle: ResizeHandle): string {
  switch (handle) {
    case 'nw': case 'se': return 'nwse-resize'
    case 'ne': case 'sw': return 'nesw-resize'
    case 'n':  case 's':  return 'ns-resize'
    case 'e':  case 'w':  return 'ew-resize'
  }
}

/** Apply a pointer-delta to `start`, producing the resized rect. `dx`
 *  and `dy` are in world units. `lockAspect` preserves the starting
 *  rect's aspect ratio (Shift during a corner drag). */
export function resizeRect(
  start: { x: number; y: number; width: number; height: number },
  handle: ResizeHandle,
  dx: number,
  dy: number,
  lockAspect: boolean,
): { x: number; y: number; width: number; height: number } {
  let x = start.x
  let y = start.y
  let w = start.width
  let h = start.height

  // Fixed-corner semantics: the corner opposite the dragged handle
  // stays put; the dragged handle follows the pointer.
  const right = start.x + start.width
  const bottom = start.y + start.height

  const touchesLeft = handle === 'nw' || handle === 'w' || handle === 'sw'
  const touchesTop = handle === 'nw' || handle === 'n' || handle === 'ne'
  const touchesRight = handle === 'ne' || handle === 'e' || handle === 'se'
  const touchesBottom = handle === 'sw' || handle === 's' || handle === 'se'

  if (touchesLeft) {
    x = start.x + dx
    w = start.width - dx
  }
  if (touchesTop) {
    y = start.y + dy
    h = start.height - dy
  }
  if (touchesRight) {
    w = start.width + dx
  }
  if (touchesBottom) {
    h = start.height + dy
  }

  if (lockAspect && handle !== 'n' && handle !== 's' && handle !== 'e' && handle !== 'w') {
    const ratio = start.width / Math.max(start.height, 1)
    // Use the larger relative delta to drive both axes.
    const relW = Math.abs(w - start.width) / Math.max(start.width, 1)
    const relH = Math.abs(h - start.height) / Math.max(start.height, 1)
    if (relW >= relH) {
      h = w / ratio
    } else {
      w = h * ratio
    }
    // Re-anchor the non-touching side so the opposite corner still
    // stays fixed after the aspect lock moves things.
    if (touchesLeft) x = right - w
    if (touchesTop) y = bottom - h
  }

  // Clamp: pin the dragged handle so we never flip through the fixed
  // corner. When min is hit on the left/top side, keep the right/bottom
  // edge fixed by recomputing x/y from the opposite edge.
  if (w < MIN_NODE_SIZE) {
    w = MIN_NODE_SIZE
    if (touchesLeft) x = right - w
  }
  if (h < MIN_NODE_SIZE) {
    h = MIN_NODE_SIZE
    if (touchesTop) y = bottom - h
  }

  return { x, y, width: w, height: h }
}

/**
 * Hit-test: return the topmost node whose bounding rect contains
 * `(worldX, worldY)`, or `null`. Non-group nodes are searched first
 * (they render above groups), in reverse iteration order so the
 * most-recently-drawn node wins — matches visual z-order.
 */
export function hitTestNode(
  nodes: readonly CanvasNode[],
  worldX: number,
  worldY: number,
): CanvasNode | null {
  for (let i = nodes.length - 1; i >= 0; i--) {
    const n = nodes[i]
    if (n.type === 'group') continue
    if (rectContains(n, worldX, worldY)) return n
  }
  // Groups last so clicking inside a group-only region still works.
  for (let i = nodes.length - 1; i >= 0; i--) {
    const n = nodes[i]
    if (n.type !== 'group') continue
    if (rectContains(n, worldX, worldY)) return n
  }
  return null
}

function rectContains(n: CanvasNode, x: number, y: number): boolean {
  return x >= n.x && x <= n.x + n.width && y >= n.y && y <= n.y + n.height
}

export function readTheme(el: HTMLElement): Theme {
  const styles = window.getComputedStyle(el)
  const read = (name: string, fallback: string) => {
    const v = styles.getPropertyValue(name).trim()
    return v || fallback
  }
  return {
    bgMuted: read('--bg-muted', FALLBACK_THEME.bgMuted),
    bgRaised: read('--bg-raised', FALLBACK_THEME.bgRaised),
    fg: read('--fg', FALLBACK_THEME.fg),
    fgMuted: read('--fg-muted', FALLBACK_THEME.fgMuted),
    accent: read('--accent', FALLBACK_THEME.accent),
    border: read('--divider-color', FALLBACK_THEME.border),
  }
}

/** Axis-aligned rect in world space. Drawn as a translucent accent
 *  overlay above nodes while a marquee drag is in progress. */
export interface MarqueeRect {
  x: number
  y: number
  width: number
  height: number
}

/** State for an in-progress drag-to-create-edge gesture: anchor on
 *  the source node's side centre, live pointer position in world
 *  space. The renderer draws a dashed preview bezier between them. */
export interface EdgeDragPreview {
  fromNodeId: string
  fromSide: NodeSide
  toWorld: { x: number; y: number }
}

export interface RenderContext {
  ctx: CanvasRenderingContext2D
  width: number
  height: number
  camera: Camera
  theme: Theme
  dpr: number
  selection?: Set<string>
  /** Id of the currently selected edge (Phase 4). Drawn thicker +
   *  accent-coloured when set so the user can see which edge the
   *  inspector is editing. */
  selectedEdgeId?: string | null
  marquee?: MarqueeRect | null
  hoveredNodeId?: string | null
  edgeDrag?: EdgeDragPreview | null
}

/** World-space rect-vs-node test: any pixel of overlap counts as a
 *  hit, matching Figma / Obsidian. Groups participate so you can
 *  marquee-pick them too. */
export function marqueeHit(nodes: readonly CanvasNode[], rect: MarqueeRect): string[] {
  const x2 = rect.x + rect.width
  const y2 = rect.y + rect.height
  const ids: string[] = []
  for (const n of nodes) {
    const nx2 = n.x + n.width
    const ny2 = n.y + n.height
    if (n.x < x2 && nx2 > rect.x && n.y < y2 && ny2 > rect.y) ids.push(n.id)
  }
  return ids
}

export type NodeSide = 'n' | 's' | 'e' | 'w'

export const NODE_SIDES: readonly NodeSide[] = ['n', 'e', 's', 'w']

/** Centre of a side of the node's rect. Used both for drawing the
 *  edge-create affordances and as the anchor for preview edges. */
export function sideCentre(node: CanvasNode, side: NodeSide): { x: number; y: number } {
  const cx = node.x + node.width / 2
  const cy = node.y + node.height / 2
  const x2 = node.x + node.width
  const y2 = node.y + node.height
  switch (side) {
    case 'n': return { x: cx, y: node.y }
    case 's': return { x: cx, y: y2 }
    case 'e': return { x: x2, y: cy }
    case 'w': return { x: node.x, y: cy }
  }
}

/** How far (in CSS pixels) the edge-create affordance sits outside the
 *  node's border — far enough to clear the mid-edge resize handle when
 *  the node happens to be single-selected. */
const EDGE_HANDLE_OFFSET_CSS = 14
/** Radius of the affordance hit circle in CSS pixels. */
export const EDGE_HANDLE_RADIUS_CSS = 7

/** Screen-space centre of the edge-create affordance for `side`. */
function edgeHandleScreenCentre(
  node: CanvasNode,
  side: NodeSide,
  camera: Camera,
): { x: number; y: number } {
  const c = sideCentre(node, side)
  const sx = (c.x - camera.x) * camera.zoom
  const sy = (c.y - camera.y) * camera.zoom
  switch (side) {
    case 'n': return { x: sx, y: sy - EDGE_HANDLE_OFFSET_CSS }
    case 's': return { x: sx, y: sy + EDGE_HANDLE_OFFSET_CSS }
    case 'e': return { x: sx + EDGE_HANDLE_OFFSET_CSS, y: sy }
    case 'w': return { x: sx - EDGE_HANDLE_OFFSET_CSS, y: sy }
  }
}

/** Hit-test the four edge-create affordances on `node` in screen
 *  space. */
export function hitTestEdgeHandle(
  node: CanvasNode,
  camera: Camera,
  screenX: number,
  screenY: number,
): NodeSide | null {
  for (const side of NODE_SIDES) {
    const c = edgeHandleScreenCentre(node, side, camera)
    const dx = screenX - c.x
    const dy = screenY - c.y
    if (dx * dx + dy * dy <= EDGE_HANDLE_RADIUS_CSS * EDGE_HANDLE_RADIUS_CSS) {
      return side
    }
  }
  return null
}

/** Return the id of the edge whose rendered curve passes within
 *  `tolerance` world units of `(worldX, worldY)`, or null. Used for
 *  click-to-select edges in Phase 4.
 *
 *  Implementation note: we sample the same cubic bezier that
 *  `drawEdge` renders and run a point-to-segment distance over the
 *  polyline. 16 samples is the smallest count that keeps a smoothly
 *  curving edge reliably clickable at 1× zoom without being too
 *  generous on orthogonal-ish edges. Callers should scale the
 *  tolerance by 1/zoom so it stays at ~6 CSS pixels at every zoom. */
export function hitTestEdge(
  doc: CanvasDoc,
  worldX: number,
  worldY: number,
  tolerance: number,
): string | null {
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const tol2 = tolerance * tolerance
  // Reverse so edges painted last (on top) win ties.
  for (let i = doc.edges.length - 1; i >= 0; i--) {
    const edge = doc.edges[i]
    const from = byId.get(edge.fromNode)
    const to = byId.get(edge.toNode)
    if (!from || !to) continue
    const start = nearestBorderPoint(from, centerOf(to))
    const end = nearestBorderPoint(to, centerOf(from))
    const midX = (start.x + end.x) / 2
    // Sample the same bezier drawEdge emits.
    let prev = start
    const steps = 16
    for (let s = 1; s <= steps; s++) {
      const t = s / steps
      const p = cubicBezier(start, { x: midX, y: start.y }, { x: midX, y: end.y }, end, t)
      if (pointSegmentDist2(worldX, worldY, prev, p) <= tol2) return edge.id
      prev = p
    }
  }
  return null
}

function cubicBezier(
  p0: { x: number; y: number },
  p1: { x: number; y: number },
  p2: { x: number; y: number },
  p3: { x: number; y: number },
  t: number,
): { x: number; y: number } {
  const mt = 1 - t
  const a = mt * mt * mt
  const b = 3 * mt * mt * t
  const c = 3 * mt * t * t
  const d = t * t * t
  return {
    x: a * p0.x + b * p1.x + c * p2.x + d * p3.x,
    y: a * p0.y + b * p1.y + c * p2.y + d * p3.y,
  }
}

function pointSegmentDist2(
  px: number,
  py: number,
  a: { x: number; y: number },
  b: { x: number; y: number },
): number {
  const dx = b.x - a.x
  const dy = b.y - a.y
  const len2 = dx * dx + dy * dy
  if (len2 === 0) {
    const ex = px - a.x
    const ey = py - a.y
    return ex * ex + ey * ey
  }
  let t = ((px - a.x) * dx + (py - a.y) * dy) / len2
  if (t < 0) t = 0
  else if (t > 1) t = 1
  const cx = a.x + t * dx
  const cy = a.y + t * dy
  const ex = px - cx
  const ey = py - cy
  return ex * ex + ey * ey
}

/** Build a marquee rect from two world-space points in any order. */
export function marqueeFromPoints(
  a: { x: number; y: number },
  b: { x: number; y: number },
): MarqueeRect {
  const x = Math.min(a.x, b.x)
  const y = Math.min(a.y, b.y)
  return { x, y, width: Math.abs(b.x - a.x), height: Math.abs(b.y - a.y) }
}

export function render(rc: RenderContext, doc: CanvasDoc | null): void {
  const { ctx, width, height, camera, theme, dpr } = rc

  // Clear in device pixels, then scale for DPR so 1 unit = 1 CSS pixel
  // for the rest of the draw.
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.clearRect(0, 0, width * dpr, height * dpr)
  ctx.fillStyle = theme.bgMuted
  ctx.fillRect(0, 0, width * dpr, height * dpr)

  ctx.scale(dpr, dpr)

  // Apply camera: world point P maps to (P - origin) * zoom.
  ctx.translate(-camera.x * camera.zoom, -camera.y * camera.zoom)
  ctx.scale(camera.zoom, camera.zoom)

  drawGrid(ctx, camera, width, height, theme)

  if (!doc) return

  // Group nodes render underneath non-group nodes.
  const groups = doc.nodes.filter((n) => n.type === 'group')
  const nonGroups = doc.nodes.filter((n) => n.type !== 'group')

  const selection = rc.selection ?? new Set<string>()

  for (const g of groups) drawNode(ctx, g, theme, selection.has(g.id))

  // Edges sit under the nodes they connect (but above groups).
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const selectedEdgeId = rc.selectedEdgeId ?? null
  for (const edge of doc.edges) {
    drawEdge(ctx, edge, byId, theme, edge.id === selectedEdgeId)
  }

  for (const n of nonGroups) drawNode(ctx, n, theme, selection.has(n.id))

  // Resize handles render only for a single selected node — matches
  // Obsidian. Multi-select shows selection outlines but no handles,
  // since resizing a group needs a separate UX pass.
  if (selection.size === 1) {
    const only = doc.nodes.find((n) => n.id === Array.from(selection)[0])
    if (only && only.type !== 'group') {
      drawResizeHandles(ctx, only, camera, theme)
    }
  }

  // Edge-create affordances show on the hovered node (unless it's a
  // group). They sit OUTSIDE the node so they don't overlap the mid-
  // edge resize handles when the node also happens to be selected.
  if (rc.hoveredNodeId && !rc.edgeDrag) {
    const hover = doc.nodes.find((n) => n.id === rc.hoveredNodeId)
    if (hover && hover.type !== 'group') {
      drawEdgeHandles(ctx, hover, camera, theme)
    }
  }

  if (rc.edgeDrag) {
    const src = doc.nodes.find((n) => n.id === rc.edgeDrag!.fromNodeId)
    if (src) drawEdgePreview(ctx, src, rc.edgeDrag.fromSide, rc.edgeDrag.toWorld, theme, camera)
  }

  if (rc.marquee && (rc.marquee.width > 0 || rc.marquee.height > 0)) {
    ctx.save()
    ctx.fillStyle = hexWithAlpha(theme.accent, 0.1)
    ctx.fillRect(rc.marquee.x, rc.marquee.y, rc.marquee.width, rc.marquee.height)
    ctx.strokeStyle = theme.accent
    ctx.lineWidth = 1 / camera.zoom
    ctx.strokeRect(rc.marquee.x, rc.marquee.y, rc.marquee.width, rc.marquee.height)
    ctx.restore()
  }
}

function drawGrid(
  ctx: CanvasRenderingContext2D,
  camera: Camera,
  cssWidth: number,
  cssHeight: number,
  theme: Theme,
): void {
  // 64-unit dot grid. Skip once we've zoomed out so far that the grid
  // becomes visual noise rather than spatial reference.
  if (camera.zoom < 0.35) return
  const step = 64
  const viewLeft = camera.x
  const viewTop = camera.y
  const viewRight = camera.x + cssWidth / camera.zoom
  const viewBottom = camera.y + cssHeight / camera.zoom
  const startX = Math.floor(viewLeft / step) * step
  const startY = Math.floor(viewTop / step) * step
  ctx.fillStyle = theme.border
  const r = 1 / camera.zoom
  for (let x = startX; x <= viewRight; x += step) {
    for (let y = startY; y <= viewBottom; y += step) {
      ctx.beginPath()
      ctx.arc(x, y, r, 0, Math.PI * 2)
      ctx.fill()
    }
  }
}

function drawNode(
  ctx: CanvasRenderingContext2D,
  node: CanvasNode,
  theme: Theme,
  selected: boolean,
): void {
  const radius = node.type === 'group' ? 4 : 8
  const fill = node.color ?? theme.bgRaised
  const stroke = selected ? theme.accent : theme.border
  const strokeWidth = selected ? 2 : 1
  const fg = theme.fg
  const muted = theme.fgMuted

  ctx.save()
  if (node.type === 'group') {
    // Translucent fill + dashed border + label tab.
    ctx.fillStyle = hexWithAlpha(fill, 0.08)
    fillRoundRect(ctx, node.x, node.y, node.width, node.height, radius)
    ctx.setLineDash([8, 6])
    ctx.strokeStyle = stroke
    ctx.lineWidth = strokeWidth
    strokeRoundRect(ctx, node.x, node.y, node.width, node.height, radius)
    ctx.setLineDash([])
    if (node.label) {
      drawLabelTab(ctx, node.x + 8, node.y - 2, node.label, fg, theme.bgMuted)
    }
    ctx.restore()
    return
  }

  ctx.fillStyle = fill
  fillRoundRect(ctx, node.x, node.y, node.width, node.height, radius)
  ctx.strokeStyle = stroke
  ctx.lineWidth = 1
  strokeRoundRect(ctx, node.x, node.y, node.width, node.height, radius)

  clipRoundRect(ctx, node.x + 1, node.y + 1, node.width - 2, node.height - 2, radius - 1)
  const pad = 10

  if (node.type === 'text') {
    // Text body is drawn by the DOM overlay layer (Phase 5a) so
    // markdown formatting + links render as real HTML. The 2D canvas
    // keeps the card background + border it already drew above, which
    // is enough to show selection/resize affordances.
  } else if (node.type === 'file') {
    const basename = basenameOf(node.file ?? '')
    ctx.fillStyle = muted
    ctx.font = '11px var(--font-monospace, ui-monospace, monospace)'
    ctx.fillText('FILE', node.x + pad, node.y + pad + 10)
    ctx.fillStyle = fg
    ctx.font = '14px var(--font-family, system-ui, sans-serif)'
    ctx.fillText(basename || '(untitled)', node.x + pad, node.y + pad + 32)
    if (node.file) {
      ctx.fillStyle = muted
      ctx.font = '11px var(--font-monospace, ui-monospace, monospace)'
      ctx.fillText(node.file, node.x + pad, node.y + pad + 50)
    }
  } else if (node.type === 'link') {
    // Link body is drawn by the DOM overlay layer (Phase 5b) so we
    // can show a live OG preview (favicon + title + description +
    // image). The 2D canvas keeps the card chrome above.
  } else if (node.type === 'database') {
    ctx.fillStyle = muted
    ctx.font = '11px var(--font-monospace, ui-monospace, monospace)'
    ctx.fillText('DATABASE', node.x + pad, node.y + pad + 10)
    ctx.fillStyle = fg
    ctx.font = '14px var(--font-family, system-ui, sans-serif)'
    ctx.fillText(node.source ?? '(no source)', node.x + pad, node.y + pad + 32)
  } else if (node.type === 'terminal') {
    ctx.fillStyle = muted
    ctx.font = '11px var(--font-monospace, ui-monospace, monospace)'
    ctx.fillText('TERMINAL', node.x + pad, node.y + pad + 10)
    ctx.fillStyle = fg
    ctx.font = '12px var(--font-monospace, ui-monospace, monospace)'
    wrapText(
      ctx,
      '$ ' + (node.command ?? ''),
      node.x + pad,
      node.y + pad + 32,
      node.width - pad * 2,
      15,
    )
  }

  ctx.restore()
}

function drawEdge(
  ctx: CanvasRenderingContext2D,
  edge: CanvasEdge,
  byId: Map<string, CanvasNode>,
  theme: Theme,
  selected: boolean,
): void {
  const from = byId.get(edge.fromNode)
  const to = byId.get(edge.toNode)
  if (!from || !to) return

  const start = nearestBorderPoint(from, centerOf(to))
  const end = nearestBorderPoint(to, centerOf(from))
  const color = selected ? theme.accent : edge.color ?? theme.fgMuted

  ctx.save()
  ctx.strokeStyle = color
  ctx.lineWidth = selected ? 2.5 : 1.5
  if (edge.type === 'dashed') ctx.setLineDash([8, 5])
  else if (edge.type === 'dotted') ctx.setLineDash([2, 4])

  ctx.beginPath()
  const midX = (start.x + end.x) / 2
  ctx.moveTo(start.x, start.y)
  // Gentle cubic bezier that bends horizontally between nodes.
  ctx.bezierCurveTo(midX, start.y, midX, end.y, end.x, end.y)
  ctx.stroke()
  ctx.setLineDash([])

  drawArrowHead(ctx, end, { x: midX, y: end.y }, color)

  if (edge.label) {
    const lx = (start.x + end.x) / 2
    const ly = (start.y + end.y) / 2
    ctx.fillStyle = theme.bgMuted
    const padX = 4
    const metrics = ctx.measureText(edge.label)
    ctx.font = '11px var(--font-family, system-ui, sans-serif)'
    const m2 = ctx.measureText(edge.label)
    const w = Math.max(metrics.width, m2.width) + padX * 2
    ctx.fillRect(lx - w / 2, ly - 9, w, 16)
    ctx.fillStyle = theme.fg
    ctx.textAlign = 'center'
    ctx.fillText(edge.label, lx, ly + 3)
    ctx.textAlign = 'start'
  }
  ctx.restore()
}

function drawResizeHandles(
  ctx: CanvasRenderingContext2D,
  node: CanvasNode,
  camera: Camera,
  theme: Theme,
): void {
  // Handles are drawn in world space but at a constant visual size
  // regardless of zoom — scale the box by 1/zoom so HANDLE_HALF_CSS
  // is preserved.
  const r = HANDLE_HALF_CSS / camera.zoom
  ctx.save()
  ctx.fillStyle = theme.bgMuted
  ctx.strokeStyle = theme.accent
  ctx.lineWidth = 1.5 / camera.zoom
  for (const h of RESIZE_HANDLES) {
    const c = handleCentre(node, h)
    ctx.fillRect(c.x - r, c.y - r, r * 2, r * 2)
    ctx.strokeRect(c.x - r, c.y - r, r * 2, r * 2)
  }
  ctx.restore()
}

function drawEdgeHandles(
  ctx: CanvasRenderingContext2D,
  node: CanvasNode,
  camera: Camera,
  theme: Theme,
): void {
  // Drawn in screen space via an inverse-zoom transform so the circles
  // always render at EDGE_HANDLE_RADIUS_CSS regardless of camera zoom.
  const r = EDGE_HANDLE_RADIUS_CSS / camera.zoom
  ctx.save()
  ctx.fillStyle = theme.bgMuted
  ctx.strokeStyle = theme.accent
  ctx.lineWidth = 1.5 / camera.zoom
  for (const side of NODE_SIDES) {
    const c = sideCentre(node, side)
    // Offset world-space position to mirror edgeHandleScreenCentre,
    // converting the CSS-pixel offset into world units via /zoom.
    const off = 14 / camera.zoom
    let cx = c.x
    let cy = c.y
    if (side === 'n') cy -= off
    else if (side === 's') cy += off
    else if (side === 'e') cx += off
    else if (side === 'w') cx -= off
    ctx.beginPath()
    ctx.arc(cx, cy, r, 0, Math.PI * 2)
    ctx.fill()
    ctx.stroke()
    // Small plus glyph.
    ctx.beginPath()
    const plusR = r * 0.5
    ctx.moveTo(cx - plusR, cy)
    ctx.lineTo(cx + plusR, cy)
    ctx.moveTo(cx, cy - plusR)
    ctx.lineTo(cx, cy + plusR)
    ctx.stroke()
  }
  ctx.restore()
}

function drawEdgePreview(
  ctx: CanvasRenderingContext2D,
  src: CanvasNode,
  fromSide: NodeSide,
  toWorld: { x: number; y: number },
  theme: Theme,
  camera: Camera,
): void {
  const start = sideCentre(src, fromSide)
  const midX = (start.x + toWorld.x) / 2
  ctx.save()
  ctx.strokeStyle = theme.accent
  ctx.lineWidth = 1.5 / camera.zoom
  ctx.setLineDash([8 / camera.zoom, 5 / camera.zoom])
  ctx.beginPath()
  ctx.moveTo(start.x, start.y)
  ctx.bezierCurveTo(midX, start.y, midX, toWorld.y, toWorld.x, toWorld.y)
  ctx.stroke()
  ctx.restore()
}

function drawArrowHead(
  ctx: CanvasRenderingContext2D,
  tip: { x: number; y: number },
  from: { x: number; y: number },
  color: string,
): void {
  const dx = tip.x - from.x
  const dy = tip.y - from.y
  const len = Math.hypot(dx, dy) || 1
  const ux = dx / len
  const uy = dy / len
  const size = 8
  const px = -uy
  const py = ux
  const base = { x: tip.x - ux * size, y: tip.y - uy * size }
  ctx.beginPath()
  ctx.moveTo(tip.x, tip.y)
  ctx.lineTo(base.x + px * size * 0.5, base.y + py * size * 0.5)
  ctx.lineTo(base.x - px * size * 0.5, base.y - py * size * 0.5)
  ctx.closePath()
  ctx.fillStyle = color
  ctx.fill()
}

function centerOf(n: CanvasNode): { x: number; y: number } {
  return { x: n.x + n.width / 2, y: n.y + n.height / 2 }
}

/** Pick the point on the rect border closest to `target`, so edges
 *  terminate at the node boundary instead of the centre. */
function nearestBorderPoint(
  n: CanvasNode,
  target: { x: number; y: number },
): { x: number; y: number } {
  const cx = n.x + n.width / 2
  const cy = n.y + n.height / 2
  const dx = target.x - cx
  const dy = target.y - cy
  if (dx === 0 && dy === 0) return { x: cx, y: cy }
  const halfW = n.width / 2
  const halfH = n.height / 2
  const tx = dx === 0 ? Infinity : halfW / Math.abs(dx)
  const ty = dy === 0 ? Infinity : halfH / Math.abs(dy)
  const t = Math.min(tx, ty)
  return { x: cx + dx * t, y: cy + dy * t }
}

function fillRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  pathRoundRect(ctx, x, y, w, h, r)
  ctx.fill()
}

function strokeRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  pathRoundRect(ctx, x, y, w, h, r)
  ctx.stroke()
}

function clipRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  pathRoundRect(ctx, x, y, w, h, r)
  ctx.clip()
}

function pathRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  const rr = Math.min(r, w / 2, h / 2)
  ctx.beginPath()
  ctx.moveTo(x + rr, y)
  ctx.arcTo(x + w, y, x + w, y + h, rr)
  ctx.arcTo(x + w, y + h, x, y + h, rr)
  ctx.arcTo(x, y + h, x, y, rr)
  ctx.arcTo(x, y, x + w, y, rr)
  ctx.closePath()
}

function wrapText(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  maxWidth: number,
  lineHeight: number,
): void {
  if (!text) return
  const paragraphs = text.split('\n')
  let cy = y
  for (const para of paragraphs) {
    const words = para.split(/\s+/)
    let line = ''
    for (const word of words) {
      const next = line ? line + ' ' + word : word
      if (ctx.measureText(next).width > maxWidth && line) {
        ctx.fillText(line, x, cy)
        cy += lineHeight
        line = word
      } else {
        line = next
      }
    }
    if (line) ctx.fillText(line, x, cy)
    cy += lineHeight
  }
}

function drawLabelTab(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  label: string,
  fg: string,
  bg: string,
): void {
  ctx.font = '11px var(--font-family, system-ui, sans-serif)'
  const w = ctx.measureText(label).width + 12
  ctx.fillStyle = bg
  ctx.fillRect(x, y - 14, w, 14)
  ctx.fillStyle = fg
  ctx.fillText(label, x + 6, y - 3)
}

function basenameOf(path: string): string {
  if (!path) return ''
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return i >= 0 ? path.slice(i + 1) : path
}

/** Best-effort hex → rgba with alpha. Falls back to the original colour
 *  if the input isn't a 6- or 3-digit hex. */
function hexWithAlpha(hex: string, alpha: number): string {
  const h = hex.trim()
  if (!h.startsWith('#')) return h
  const body = h.slice(1)
  let r: number, g: number, b: number
  if (body.length === 3) {
    r = parseInt(body[0] + body[0], 16)
    g = parseInt(body[1] + body[1], 16)
    b = parseInt(body[2] + body[2], 16)
  } else if (body.length === 6) {
    r = parseInt(body.slice(0, 2), 16)
    g = parseInt(body.slice(2, 4), 16)
    b = parseInt(body.slice(4, 6), 16)
  } else {
    return h
  }
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}
