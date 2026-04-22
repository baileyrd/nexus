// Phase-2 canvas surface: <canvas>-based renderer with camera
// (wheel-zoom anchored on cursor, drag-to-pan). Nodes render as typed
// cards, edges as bezier lines with arrow heads. Hit-testing + drag +
// create + inspector are Phase 3; full node-body embeds are Phase 5.

import { useEffect, useRef } from 'react'
import { useCanvasStore, MIN_ZOOM, MAX_ZOOM, type Camera } from './canvasStore'
import { render, readTheme } from './renderer'
import type { CanvasKernelClient, CanvasDoc } from './kernelClient'

interface Props {
  relpath: string
  client: CanvasKernelClient
}

export function CanvasView({ relpath, client }: Props) {
  const tab = useCanvasStore((s) => s.tabs.get(relpath))
  const containerRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  // Cached refs so RAF doesn't allocate per frame and event handlers see
  // the latest values without re-binding.
  const docRef = useRef<CanvasDoc | null>(null)
  const cameraRef = useRef<Camera>({ x: 0, y: 0, zoom: 1 })
  const cameraDirtyRef = useRef(false)

  useEffect(() => {
    const store = useCanvasStore.getState()
    if (store.tabs.has(relpath)) return
    store.setLoading(relpath)
    void (async () => {
      try {
        const doc = await client.read(relpath)
        useCanvasStore.getState().setDoc(relpath, doc)
      } catch (err) {
        useCanvasStore.getState().setError(relpath, String(err))
      }
    })()
  }, [relpath, client])

  // Keep local refs in sync with store (RAF reads the refs, not zustand).
  useEffect(() => {
    docRef.current = tab?.doc ?? null
    if (tab?.camera) cameraRef.current = tab.camera
  }, [tab])

  // Zoom-to-fit the document on first render after it loads.
  useEffect(() => {
    if (!tab?.doc || tab.cameraInitialized) return
    const canvas = canvasRef.current
    const container = containerRef.current
    if (!canvas || !container) return
    const rect = container.getBoundingClientRect()
    const fit = fitCameraToDoc(tab.doc, rect.width, rect.height)
    cameraRef.current = fit
    useCanvasStore.getState().setCamera(relpath, fit)
  }, [tab?.doc, tab?.cameraInitialized, relpath])

  // RAF-driven render loop. One loop per mount; tears down on unmount.
  useEffect(() => {
    const canvas = canvasRef.current
    const container = containerRef.current
    if (!canvas || !container) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    let raf = 0
    let stopped = false
    let lastWidth = 0
    let lastHeight = 0
    let dpr = window.devicePixelRatio || 1
    const theme = readTheme(container)

    const resize = () => {
      const rect = container.getBoundingClientRect()
      dpr = window.devicePixelRatio || 1
      const w = Math.max(1, Math.floor(rect.width))
      const h = Math.max(1, Math.floor(rect.height))
      if (w !== lastWidth || h !== lastHeight) {
        canvas.width = w * dpr
        canvas.height = h * dpr
        canvas.style.width = w + 'px'
        canvas.style.height = h + 'px'
        lastWidth = w
        lastHeight = h
      }
    }

    const tick = () => {
      if (stopped) return
      resize()
      render(
        {
          ctx,
          width: lastWidth,
          height: lastHeight,
          camera: cameraRef.current,
          theme,
          dpr,
        },
        docRef.current,
      )
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)

    const ro = new ResizeObserver(resize)
    ro.observe(container)

    return () => {
      stopped = true
      cancelAnimationFrame(raf)
      ro.disconnect()
    }
  }, [])

  // Persist camera changes to the store on idle frames. We batch through
  // a dirty flag so zoom/pan doesn't thrash the zustand subscribers every
  // pointer move.
  useEffect(() => {
    let raf = 0
    const tick = () => {
      if (cameraDirtyRef.current) {
        cameraDirtyRef.current = false
        useCanvasStore.getState().setCamera(relpath, cameraRef.current)
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [relpath])

  // ── Input ────────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return

    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      const rect = canvas.getBoundingClientRect()
      const cx = e.clientX - rect.left
      const cy = e.clientY - rect.top
      const cam = cameraRef.current
      // Ctrl/cmd + wheel and pinch (ctrlKey set by browsers on trackpad
      // pinch) both zoom; plain wheel is scroll/pan.
      if (e.ctrlKey || e.metaKey) {
        // Zoom anchored on pointer: keep the world point under the
        // cursor fixed across the zoom change.
        const worldX = cam.x + cx / cam.zoom
        const worldY = cam.y + cy / cam.zoom
        const factor = Math.exp(-e.deltaY * 0.002)
        const nextZoom = clamp(cam.zoom * factor, MIN_ZOOM, MAX_ZOOM)
        cameraRef.current = {
          x: worldX - cx / nextZoom,
          y: worldY - cy / nextZoom,
          zoom: nextZoom,
        }
      } else {
        cameraRef.current = {
          x: cam.x + e.deltaX / cam.zoom,
          y: cam.y + e.deltaY / cam.zoom,
          zoom: cam.zoom,
        }
      }
      cameraDirtyRef.current = true
    }

    let panning = false
    let lastX = 0
    let lastY = 0
    const onPointerDown = (e: PointerEvent) => {
      // Middle click = dedicated pan; left click with space also pans
      // later (Phase 3 adds selection). For Phase 2, left-click-drag on
      // empty space pans too — there's no selection to conflict with.
      const middle = e.button === 1
      const left = e.button === 0
      if (!middle && !left) return
      panning = true
      lastX = e.clientX
      lastY = e.clientY
      canvas.setPointerCapture(e.pointerId)
      canvas.style.cursor = 'grabbing'
    }
    const onPointerMove = (e: PointerEvent) => {
      if (!panning) return
      const dx = e.clientX - lastX
      const dy = e.clientY - lastY
      lastX = e.clientX
      lastY = e.clientY
      const cam = cameraRef.current
      cameraRef.current = {
        x: cam.x - dx / cam.zoom,
        y: cam.y - dy / cam.zoom,
        zoom: cam.zoom,
      }
      cameraDirtyRef.current = true
    }
    const onPointerUp = (e: PointerEvent) => {
      if (!panning) return
      panning = false
      canvas.releasePointerCapture(e.pointerId)
      canvas.style.cursor = 'grab'
    }

    canvas.addEventListener('wheel', onWheel, { passive: false })
    canvas.addEventListener('pointerdown', onPointerDown)
    canvas.addEventListener('pointermove', onPointerMove)
    canvas.addEventListener('pointerup', onPointerUp)
    canvas.addEventListener('pointercancel', onPointerUp)
    canvas.style.cursor = 'grab'

    return () => {
      canvas.removeEventListener('wheel', onWheel)
      canvas.removeEventListener('pointerdown', onPointerDown)
      canvas.removeEventListener('pointermove', onPointerMove)
      canvas.removeEventListener('pointerup', onPointerUp)
      canvas.removeEventListener('pointercancel', onPointerUp)
    }
  }, [])

  const doc = tab?.doc
  const nodeCount = doc?.nodes.length ?? 0
  const edgeCount = doc?.edges.length ?? 0

  return (
    <div
      ref={containerRef}
      style={{
        position: 'relative',
        width: '100%',
        height: '100%',
        overflow: 'hidden',
      }}
    >
      <canvas
        ref={canvasRef}
        style={{
          display: 'block',
          width: '100%',
          height: '100%',
          touchAction: 'none',
        }}
      />
      {tab?.loading && <CornerLabel>Loading…</CornerLabel>}
      {tab?.error && <CornerLabel>Error: {tab.error}</CornerLabel>}
      {!tab?.loading && !tab?.error && (
        <CornerLabel>
          {nodeCount} node{nodeCount === 1 ? '' : 's'} · {edgeCount} edge
          {edgeCount === 1 ? '' : 's'} · {Math.round((tab?.camera.zoom ?? 1) * 100)}%
        </CornerLabel>
      )}
    </div>
  )
}

function CornerLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        position: 'absolute',
        top: 8,
        right: 12,
        fontSize: 12,
        color: 'var(--fg-muted, #9ca3af)',
        fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
        pointerEvents: 'none',
      }}
    >
      {children}
    </div>
  )
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v
}

/** Compute a camera that centres every node within a small margin. */
function fitCameraToDoc(doc: CanvasDoc, viewW: number, viewH: number): Camera {
  if (doc.nodes.length === 0) return { x: -viewW / 2, y: -viewH / 2, zoom: 1 }
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const n of doc.nodes) {
    if (n.x < minX) minX = n.x
    if (n.y < minY) minY = n.y
    if (n.x + n.width > maxX) maxX = n.x + n.width
    if (n.y + n.height > maxY) maxY = n.y + n.height
  }
  const padding = 80
  const contentW = maxX - minX + padding * 2
  const contentH = maxY - minY + padding * 2
  const zoom = clamp(
    Math.min(viewW / contentW, viewH / contentH, 1),
    MIN_ZOOM,
    MAX_ZOOM,
  )
  const cx = (minX + maxX) / 2
  const cy = (minY + maxY) / 2
  return {
    x: cx - viewW / 2 / zoom,
    y: cy - viewH / 2 / zoom,
    zoom,
  }
}
