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

export interface RenderContext {
  ctx: CanvasRenderingContext2D
  width: number
  height: number
  camera: Camera
  theme: Theme
  dpr: number
  selection?: Set<string>
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
  for (const edge of doc.edges) drawEdge(ctx, edge, byId, theme)

  for (const n of nonGroups) drawNode(ctx, n, theme, selection.has(n.id))
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
    ctx.fillStyle = fg
    ctx.font = '13px var(--font-family, system-ui, sans-serif)'
    wrapText(ctx, node.text ?? '', node.x + pad, node.y + pad + 12, node.width - pad * 2, 16)
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
    ctx.fillStyle = muted
    ctx.font = '11px var(--font-monospace, ui-monospace, monospace)'
    ctx.fillText('LINK', node.x + pad, node.y + pad + 10)
    ctx.fillStyle = theme.accent
    ctx.font = '13px var(--font-family, system-ui, sans-serif)'
    wrapText(
      ctx,
      node.url ?? '(no url)',
      node.x + pad,
      node.y + pad + 32,
      node.width - pad * 2,
      16,
    )
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
): void {
  const from = byId.get(edge.fromNode)
  const to = byId.get(edge.toNode)
  if (!from || !to) return

  const start = nearestBorderPoint(from, centerOf(to))
  const end = nearestBorderPoint(to, centerOf(from))
  const color = edge.color ?? theme.fgMuted

  ctx.save()
  ctx.strokeStyle = color
  ctx.lineWidth = 1.5
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
