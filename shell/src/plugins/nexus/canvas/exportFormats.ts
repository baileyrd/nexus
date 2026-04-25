// Rich exporters for the canvas surface. The Phase-6 first cut only
// captured the 2D layer (see exportPng.ts); this module snapshots the
// container — 2D canvas + DOM overlay together — using html-to-image,
// so markdown bodies, OG cards, mini-base grids, PTY transcripts, and
// everything else rendered through CanvasOverlay all end up in the
// exported image. PDF wraps the same raster through jspdf.
//
// The 2D-only PNG exporter stays for the "Export" button until the
// UI menu grows the three-format picker.

import { toPng, toSvg } from 'html-to-image'
import jsPDF from 'jspdf'
import type { CanvasDoc } from './kernelClient'
import { contentBounds } from './renderer'
import { configStore } from '../../../stores/configStore'

/** Max pixel dimension per edge to keep exports manageable. Mirrors
 *  exportPng.ts. */
const MAX_EXPORT_EDGE_PX = 8192
const EXPORT_MARGIN_UNITS = 48

/** Shared bbox / clip math. Produces the container-relative rectangle
 *  we want to capture plus the device-pixel dimensions for the
 *  output. Returns null when there are no nodes. */
function captureGeometry(
  doc: CanvasDoc,
  container: HTMLElement,
): { width: number; height: number; scale: number; bbox: ReturnType<typeof contentBounds> } | null {
  const bbox = contentBounds(doc)
  if (!bbox) return null
  const marginUnits = configStore.get('canvas.exportMarginUnits', EXPORT_MARGIN_UNITS) ?? EXPORT_MARGIN_UNITS
  const maxEdgePx = configStore.get('canvas.maxExportEdge', MAX_EXPORT_EDGE_PX) ?? MAX_EXPORT_EDGE_PX
  const w = bbox.width + marginUnits * 2
  const h = bbox.height + marginUnits * 2
  const rect = container.getBoundingClientRect()
  // Base scale: map world units to CSS px at the leaf's current size.
  const fit = Math.min(rect.width / w, rect.height / h) || 1
  let pixelW = Math.round(w * fit)
  let pixelH = Math.round(h * fit)
  if (pixelW > maxEdgePx || pixelH > maxEdgePx) {
    const k = Math.min(maxEdgePx / pixelW, maxEdgePx / pixelH)
    pixelW = Math.round(pixelW * k)
    pixelH = Math.round(pixelH * k)
  }
  return { width: pixelW, height: pixelH, scale: fit, bbox }
}

/** Capture the live container node. `pixelRatio` controls raster
 *  density; `filter` drops elements we know we don't want (control
 *  strip, minimap, inspector — everything the user wouldn't want in
 *  a saved copy). */
async function snapshotContainer(
  container: HTMLElement,
  producer: (el: HTMLElement, opts: Parameters<typeof toPng>[1]) => Promise<string>,
  pixelRatio: number,
): Promise<string | null> {
  const opts: Parameters<typeof toPng>[1] = {
    pixelRatio,
    cacheBust: true,
    filter: (node) => {
      if (!(node instanceof HTMLElement)) return true
      // Exclude chrome overlays the user explicitly hid before
      // exporting. The data-attribute is set by ControlStrip /
      // Minimap / Inspector roots below.
      return node.dataset.canvasExportExclude !== 'true'
    },
  }
  try {
    return await producer(container, opts)
  } catch (err) {
    console.warn('[nexus.canvas] html-to-image snapshot failed:', err)
    return null
  }
}

/** Overlay-inclusive PNG export — raster that matches what the user
 *  sees on screen (minus control-strip / minimap / inspector). */
export async function exportCanvasPng(
  doc: CanvasDoc,
  container: HTMLElement,
): Promise<Blob | null> {
  const geom = captureGeometry(doc, container)
  if (!geom) return null
  const ratio = Math.max(1, geom.width / container.getBoundingClientRect().width)
  const dataUrl = await snapshotContainer(container, toPng, ratio)
  if (!dataUrl) return null
  return dataUrlToBlob(dataUrl)
}

/** SVG export — produces a `foreignObject`-wrapped XML document via
 *  html-to-image. Fidelity varies by browser's SVG support for
 *  arbitrary HTML / canvas elements. Falls back to PNG-inside-SVG
 *  when the browser doesn't honour the foreign content. */
export async function exportCanvasSvg(
  doc: CanvasDoc,
  container: HTMLElement,
): Promise<Blob | null> {
  const geom = captureGeometry(doc, container)
  if (!geom) return null
  const ratio = Math.max(1, geom.width / container.getBoundingClientRect().width)
  const dataUrl = await snapshotContainer(container, toSvg, ratio)
  if (!dataUrl) return null
  return dataUrlToBlob(dataUrl)
}

/** PDF export — renders the overlay-inclusive raster into a single
 *  page sized to the raster dimensions. Uses portrait or landscape
 *  based on the bbox aspect ratio. */
export async function exportCanvasPdf(
  doc: CanvasDoc,
  container: HTMLElement,
): Promise<Blob | null> {
  const geom = captureGeometry(doc, container)
  if (!geom) return null
  const ratio = Math.max(1, geom.width / container.getBoundingClientRect().width)
  const dataUrl = await snapshotContainer(container, toPng, ratio)
  if (!dataUrl) return null
  const orientation = geom.width >= geom.height ? 'landscape' : 'portrait'
  const pdf = new jsPDF({
    orientation,
    unit: 'px',
    format: [geom.width, geom.height],
    compress: true,
  })
  pdf.addImage(dataUrl, 'PNG', 0, 0, geom.width, geom.height, undefined, 'FAST')
  return pdf.output('blob')
}

function dataUrlToBlob(dataUrl: string): Blob {
  const [meta, b64] = dataUrl.split(',', 2)
  const mime = /data:([^;]+)/.exec(meta)?.[1] ?? 'application/octet-stream'
  // SVG payloads are URL-encoded, not base64, when emitted by toSvg.
  if (!meta.includes('base64')) {
    const decoded = decodeURIComponent(b64)
    return new Blob([decoded], { type: mime })
  }
  const bin = atob(b64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return new Blob([bytes], { type: mime })
}
