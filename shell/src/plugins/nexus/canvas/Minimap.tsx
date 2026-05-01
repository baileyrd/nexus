// Phase-6 minimap: a small overview canvas that renders every node
// as a filled rect plus a stroked rectangle for the current viewport.
// Click/drag inside the minimap pans the camera so the clicked point
// becomes the new viewport centre.
//
// Implementation notes:
// - Own RAF loop so we re-render alongside the main canvas without
//   going through zustand (the main canvas already owns the hot
//   path; the minimap piggybacks off the same refs CanvasView keeps).
// - Pure 2D — no DOM overlay needed; node bodies don't render into
//   the minimap, only card rectangles.
// - Fit-to-content scale with padding, so a lopsided canvas still
//   shows everything without black bars eating half the strip.

import { useEffect, useRef, forwardRef, useImperativeHandle } from 'react'
import type { CanvasDoc } from './kernelClient'
import type { Camera } from './canvasStore'
import { contentBounds } from './renderer'

/** CSS size of the minimap widget. World-space content is scaled to
 *  fit inside these bounds with `MINIMAP_PADDING` cushioning. */
const MINIMAP_WIDTH = 200
const MINIMAP_HEIGHT = 140
/** Empty pixels kept between the content bbox and the minimap's
 *  border so the viewport frame stays visible when the camera is
 *  parked at the extreme edge of content. */
const MINIMAP_PADDING = 8

/** Imperative API the parent uses to refresh the minimap whenever
 *  its inputs change. We avoid a React re-render so minimap frames
 *  don't compete with the main RAF loop for reconciler time.*/
export interface MinimapHandle {
  /** Re-render with the current doc + camera + viewport. Called
   *  from CanvasView's main RAF tick. */
  redraw(doc: CanvasDoc | null, camera: Camera, viewport: { w: number; h: number }): void
}

interface Props {
  /** Pan the parent camera so `(worldX, worldY)` sits at the centre
   *  of the main viewport. */
  onRecenter: (worldX: number, worldY: number) => void
}

export const Minimap = forwardRef<MinimapHandle, Props>(function Minimap(
  { onRecenter },
  ref,
) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  // Cached inputs so a click handler can translate screen → world
  // without waiting for the next redraw call.
  const lastRef = useRef<{
    doc: CanvasDoc | null
    bounds: { x: number; y: number; width: number; height: number } | null
    scale: number
    offsetX: number
    offsetY: number
  }>({ doc: null, bounds: null, scale: 1, offsetX: 0, offsetY: 0 })

  useImperativeHandle(ref, () => ({
    redraw(doc, camera, viewport) {
      const canvas = canvasRef.current
      if (!canvas) return
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      const dpr = window.devicePixelRatio || 1
      // Size once per dpr change — cheaper than per-frame resize.
      const wantW = MINIMAP_WIDTH * dpr
      const wantH = MINIMAP_HEIGHT * dpr
      if (canvas.width !== wantW) canvas.width = wantW
      if (canvas.height !== wantH) canvas.height = wantH
      ctx.setTransform(1, 0, 0, 1, 0, 0)
      ctx.clearRect(0, 0, wantW, wantH)
      ctx.scale(dpr, dpr)

      // Background
      const styles = window.getComputedStyle(canvas)
      const read = (...names: string[]) => {
        for (const n of names) {
          const v = styles.getPropertyValue(n).trim()
          if (v) return v
        }
        return ''
      }
      const bg       = read('--background-primary',        '--bg-muted')
      const fg       = read('--text-muted',                '--text-muted')
      const accent   = read('--interactive-accent',        '--interactive-accent')
      const nodeFill = read('--background-secondary',      '--background-secondary')
      ctx.fillStyle = bg
      ctx.fillRect(0, 0, MINIMAP_WIDTH, MINIMAP_HEIGHT)

      const bounds = doc ? contentBounds(doc) : null
      if (!bounds || !doc) {
        ctx.fillStyle = fg
        ctx.font = '10px var(--font-family, system-ui, sans-serif)'
        ctx.textAlign = 'center'
        ctx.fillText('empty', MINIMAP_WIDTH / 2, MINIMAP_HEIGHT / 2)
        ctx.textAlign = 'start'
        lastRef.current = { doc, bounds: null, scale: 1, offsetX: 0, offsetY: 0 }
        return
      }
      // Union content bounds + the current viewport so panning off-
      // content still shows where the user is looking.
      const viewBounds = {
        x: camera.x,
        y: camera.y,
        width: viewport.w / camera.zoom,
        height: viewport.h / camera.zoom,
      }
      const unionX = Math.min(bounds.x, viewBounds.x)
      const unionY = Math.min(bounds.y, viewBounds.y)
      const unionW = Math.max(bounds.x + bounds.width, viewBounds.x + viewBounds.width) - unionX
      const unionH =
        Math.max(bounds.y + bounds.height, viewBounds.y + viewBounds.height) - unionY

      const availW = MINIMAP_WIDTH - MINIMAP_PADDING * 2
      const availH = MINIMAP_HEIGHT - MINIMAP_PADDING * 2
      const scale = Math.min(availW / unionW, availH / unionH)
      const offsetX = MINIMAP_PADDING + (availW - unionW * scale) / 2 - unionX * scale
      const offsetY = MINIMAP_PADDING + (availH - unionH * scale) / 2 - unionY * scale

      const toX = (wx: number) => offsetX + wx * scale
      const toY = (wy: number) => offsetY + wy * scale

      // Nodes
      for (const n of doc.nodes) {
        if (n.type === 'group') {
          ctx.fillStyle = 'rgba(255,255,255,0.04)'
          ctx.fillRect(toX(n.x), toY(n.y), n.width * scale, n.height * scale)
          continue
        }
        ctx.fillStyle = n.color ?? nodeFill
        ctx.fillRect(toX(n.x), toY(n.y), Math.max(1, n.width * scale), Math.max(1, n.height * scale))
      }

      // Viewport frame
      ctx.strokeStyle = accent
      ctx.lineWidth = 1
      ctx.strokeRect(
        toX(viewBounds.x),
        toY(viewBounds.y),
        viewBounds.width * scale,
        viewBounds.height * scale,
      )

      lastRef.current = { doc, bounds, scale, offsetX, offsetY }
    },
  }))

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const rectToWorld = (clientX: number, clientY: number) => {
      const rect = canvas.getBoundingClientRect()
      const cssX = clientX - rect.left
      const cssY = clientY - rect.top
      const { scale, offsetX, offsetY } = lastRef.current
      if (scale <= 0) return null
      return {
        x: (cssX - offsetX) / scale,
        y: (cssY - offsetY) / scale,
      }
    }
    let dragging = false
    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return
      const w = rectToWorld(e.clientX, e.clientY)
      if (!w) return
      dragging = true
      canvas.setPointerCapture(e.pointerId)
      onRecenter(w.x, w.y)
    }
    const onPointerMove = (e: PointerEvent) => {
      if (!dragging) return
      const w = rectToWorld(e.clientX, e.clientY)
      if (w) onRecenter(w.x, w.y)
    }
    const onPointerUp = (e: PointerEvent) => {
      dragging = false
      try {
        canvas.releasePointerCapture(e.pointerId)
      } catch {
        /* capture may already be gone */
      }
    }
    canvas.addEventListener('pointerdown', onPointerDown)
    canvas.addEventListener('pointermove', onPointerMove)
    canvas.addEventListener('pointerup', onPointerUp)
    canvas.addEventListener('pointercancel', onPointerUp)
    return () => {
      canvas.removeEventListener('pointerdown', onPointerDown)
      canvas.removeEventListener('pointermove', onPointerMove)
      canvas.removeEventListener('pointerup', onPointerUp)
      canvas.removeEventListener('pointercancel', onPointerUp)
    }
  }, [onRecenter])

  return (
    <canvas
      ref={canvasRef}
      width={MINIMAP_WIDTH}
      height={MINIMAP_HEIGHT}
      data-canvas-export-exclude="true"
      style={{
        position: 'absolute',
        top: 12,
        left: 12,
        width: MINIMAP_WIDTH,
        height: MINIMAP_HEIGHT,
        borderRadius: 6,
        border: '1px solid var(--divider-color, var(--background-modifier-border))',
        boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
        cursor: 'crosshair',
        // Minimap is interactive, unlike the passive overlay layer.
        pointerEvents: 'auto',
      }}
    />
  )
})
