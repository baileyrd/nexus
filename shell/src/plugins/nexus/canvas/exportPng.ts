// Phase-6 PNG export. Re-renders the canvas into an offscreen
// <canvas> sized to the content bbox at a fixed zoom, then triggers
// a file download.
//
// Limitation: the DOM overlay layer (markdown text, file/link/db/
// terminal embeds) is NOT captured — only what the 2D renderer
// draws (card chrome, edges, grid, selection, groups). A faithful
// export of overlay content would need html2canvas or a parallel
// SVG renderer, both of which are out of scope for the first
// polish pass. The PNG is still useful for sharing graph structure.

import { render, readTheme, contentBounds } from './renderer'
import type { CanvasDoc } from './kernelClient'

/** Margin around the content bbox in the exported image, in world
 *  units. A little breathing room so nodes don't kiss the edge. */
const EXPORT_MARGIN = 48
/** Hard cap on the rendered image edge so a 50 000 × 50 000 canvas
 *  doesn't OOM the browser. Pixels scale down uniformly past this. */
const MAX_EXPORT_EDGE = 8192

/**
 * Render `doc` into a PNG blob fitting its content bounds plus a
 * margin. `hostEl` is used to read theme CSS variables — pass the
 * canvas container so colours match what's on screen.
 *
 * Returns `null` when the doc has no nodes (nothing to export).
 */
export async function exportCanvasPng(
  doc: CanvasDoc,
  hostEl: HTMLElement,
): Promise<Blob | null> {
  const bounds = contentBounds(doc)
  if (!bounds) return null

  const theme = readTheme(hostEl)
  // Pad the bbox so nodes don't sit flush against the edge.
  const padded = {
    x: bounds.x - EXPORT_MARGIN,
    y: bounds.y - EXPORT_MARGIN,
    width: bounds.width + EXPORT_MARGIN * 2,
    height: bounds.height + EXPORT_MARGIN * 2,
  }
  // Cap the longest edge to MAX_EXPORT_EDGE; uniform scale everywhere else.
  const scale = Math.min(
    1,
    MAX_EXPORT_EDGE / Math.max(padded.width, padded.height),
  )
  const widthPx = Math.max(1, Math.round(padded.width * scale))
  const heightPx = Math.max(1, Math.round(padded.height * scale))

  const canvas = document.createElement('canvas')
  canvas.width = widthPx
  canvas.height = heightPx
  const ctx = canvas.getContext('2d')
  if (!ctx) return null

  render(
    {
      ctx,
      width: widthPx,
      height: heightPx,
      camera: { x: padded.x, y: padded.y, zoom: scale },
      theme,
      // DPR is baked into our chosen pixel size already — drive the
      // render at 1:1 so we don't double-scale.
      dpr: 1,
      selection: new Set(),
      selectedEdgeId: null,
      marquee: null,
      hoveredNodeId: null,
      edgeDrag: null,
      // Hide the grid in exports so the image reads as a
      // presentation artifact, not a canvas screenshot.
      showGrid: false,
    },
    doc,
  )

  return new Promise<Blob | null>((resolve) => {
    canvas.toBlob((blob) => resolve(blob), 'image/png')
  })
}

/**
 * Trigger a browser "save as" for `blob` under `filename`. Creates
 * and immediately revokes an object URL so the blob is GC-eligible
 * as soon as the download dialog resolves.
 */
export function triggerDownload(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  a.remove()
  // A microtask is plenty — the download reader has already captured
  // the blob reference by the time the synchronous `click()` returns.
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
