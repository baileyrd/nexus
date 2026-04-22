// Phase-3 canvas surface: selection, drag-to-move, delete, and
// double-click-to-create-text. Changes flush through canvas_patch so
// the SQLite index + knowledge graph stay in sync.
//
// Deferred to a later Phase-3 cut: shift-click multi-select, marquee
// selection, resize handles, drag-from-edge create, undo/redo stack.

import { useEffect, useRef } from 'react'
import {
  useCanvasStore,
  MIN_ZOOM,
  MAX_ZOOM,
  newNodeId,
  applyNodeMove,
  applyNodeAdd,
  applyNodeRemove,
  type Camera,
} from './canvasStore'
import { render, readTheme, hitTestNode, DEFAULT_TEXT_NODE_SIZE } from './renderer'
import type {
  CanvasKernelClient,
  CanvasDoc,
  CanvasNode,
  CanvasPatchOp,
} from './kernelClient'

interface Props {
  relpath: string
  client: CanvasKernelClient
}

/** Clicks within this many CSS pixels of pointerdown are a click, not
 *  a drag. Prevents microscopic cursor jitter from kicking the view
 *  into move-mode. */
const DRAG_THRESHOLD_PX = 3

export function CanvasView({ relpath, client }: Props) {
  const tab = useCanvasStore((s) => s.tabs.get(relpath))
  const containerRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  // Cached refs so RAF doesn't allocate per frame and event handlers see
  // the latest values without re-binding.
  const docRef = useRef<CanvasDoc | null>(null)
  const cameraRef = useRef<Camera>({ x: 0, y: 0, zoom: 1 })
  const cameraDirtyRef = useRef(false)
  const selectionRef = useRef<Set<string>>(new Set())
  const clientRef = useRef(client)
  clientRef.current = client

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
    selectionRef.current = tab?.selection ?? new Set()
  }, [tab])

  // Zoom-to-fit the document once both (a) the doc has loaded and (b)
  // the container has a non-zero layout size. The size gate matters on
  // the first mount of a tab that was opened into an off-screen leaf —
  // if we fit against a 0×0 viewport, the clamp pins zoom to 10% and
  // the content disappears offscreen. We poll via RAF until the layout
  // settles, then fire once.
  useEffect(() => {
    if (!tab?.doc || tab.cameraInitialized) return
    const container = containerRef.current
    if (!container) return
    let raf = 0
    let cancelled = false
    const tryFit = () => {
      if (cancelled) return
      const rect = container.getBoundingClientRect()
      if (rect.width < 1 || rect.height < 1) {
        raf = requestAnimationFrame(tryFit)
        return
      }
      const doc = tab.doc
      if (!doc) return
      const fit = fitCameraToDoc(doc, rect.width, rect.height)
      cameraRef.current = fit
      useCanvasStore.getState().setCamera(relpath, fit)
    }
    raf = requestAnimationFrame(tryFit)
    return () => {
      cancelled = true
      cancelAnimationFrame(raf)
    }
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
          selection: selectionRef.current,
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

  // ── Pointer input ────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return

    const screenToWorld = (cx: number, cy: number) => {
      const cam = cameraRef.current
      return { x: cam.x + cx / cam.zoom, y: cam.y + cy / cam.zoom }
    }

    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      const rect = canvas.getBoundingClientRect()
      const cx = e.clientX - rect.left
      const cy = e.clientY - rect.top
      const cam = cameraRef.current
      if (e.ctrlKey || e.metaKey) {
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

    type DragMode =
      | { kind: 'none' }
      | { kind: 'pan'; lastX: number; lastY: number }
      | {
          kind: 'move-node'
          nodeId: string
          startWorldX: number
          startWorldY: number
          startNodeX: number
          startNodeY: number
          armed: boolean
          /** Screen-pixel pointer position at pointerdown — used for
           *  DRAG_THRESHOLD_PX so a noisy click doesn't trigger a move
           *  and a zero-delta patch. */
          downCX: number
          downCY: number
        }
    let drag: DragMode = { kind: 'none' }

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0 && e.button !== 1) return
      const rect = canvas.getBoundingClientRect()
      const cx = e.clientX - rect.left
      const cy = e.clientY - rect.top
      const world = screenToWorld(cx, cy)
      const doc = docRef.current
      const hit = doc ? hitTestNode(doc.nodes, world.x, world.y) : null

      // Middle click OR empty-space click → pan.
      if (e.button === 1 || !hit) {
        if (!hit && e.button === 0) {
          // Clear selection when clicking empty space.
          useCanvasStore.getState().setSelection(relpath, [])
        }
        drag = { kind: 'pan', lastX: e.clientX, lastY: e.clientY }
        canvas.setPointerCapture(e.pointerId)
        canvas.style.cursor = 'grabbing'
        return
      }

      // Node click → select + arm a move drag (don't commit to
      // move-mode until the pointer clears DRAG_THRESHOLD_PX).
      useCanvasStore.getState().setSelection(relpath, [hit.id])
      drag = {
        kind: 'move-node',
        nodeId: hit.id,
        startWorldX: world.x,
        startWorldY: world.y,
        startNodeX: hit.x,
        startNodeY: hit.y,
        armed: false,
        downCX: cx,
        downCY: cy,
      }
      canvas.setPointerCapture(e.pointerId)
    }

    const onPointerMove = (e: PointerEvent) => {
      if (drag.kind === 'none') return
      if (drag.kind === 'pan') {
        const dx = e.clientX - drag.lastX
        const dy = e.clientY - drag.lastY
        drag.lastX = e.clientX
        drag.lastY = e.clientY
        const cam = cameraRef.current
        cameraRef.current = {
          x: cam.x - dx / cam.zoom,
          y: cam.y - dy / cam.zoom,
          zoom: cam.zoom,
        }
        cameraDirtyRef.current = true
        return
      }
      // move-node
      const rect = canvas.getBoundingClientRect()
      const cx = e.clientX - rect.left
      const cy = e.clientY - rect.top
      if (!drag.armed) {
        const ddx = cx - drag.downCX
        const ddy = cy - drag.downCY
        if (Math.hypot(ddx, ddy) < DRAG_THRESHOLD_PX) return
        drag.armed = true
        canvas.style.cursor = 'grabbing'
      }
      const world = screenToWorld(cx, cy)
      const nx = drag.startNodeX + (world.x - drag.startWorldX)
      const ny = drag.startNodeY + (world.y - drag.startWorldY)
      useCanvasStore.getState().updateDoc(relpath, (doc) => applyNodeMove(doc, drag.kind === 'move-node' ? drag.nodeId : '', nx, ny))
    }

    const onPointerUp = (e: PointerEvent) => {
      if (drag.kind === 'none') return
      try {
        canvas.releasePointerCapture(e.pointerId)
      } catch {
        // capture may already be released if focus moved away
      }
      const finished = drag
      drag = { kind: 'none' }
      canvas.style.cursor = 'grab'

      if (finished.kind === 'move-node' && finished.armed) {
        const doc = docRef.current
        const node = doc?.nodes.find((n) => n.id === finished.nodeId)
        if (!node) return
        // Only flush if the move actually changed the position.
        if (node.x === finished.startNodeX && node.y === finished.startNodeY) return
        void clientRef.current
          .patch(relpath, [
            { op: 'node_move', id: finished.nodeId, x: node.x, y: node.y },
          ])
          .catch((err) => {
            console.warn('[nexus.canvas] node_move patch failed:', err)
          })
      }
    }

    const onDoubleClick = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect()
      const cx = e.clientX - rect.left
      const cy = e.clientY - rect.top
      const world = screenToWorld(cx, cy)
      const doc = docRef.current
      if (!doc) return
      if (hitTestNode(doc.nodes, world.x, world.y)) return // on-node dbl-click is a future editor trigger
      const { width, height } = DEFAULT_TEXT_NODE_SIZE
      const node: CanvasNode = {
        id: newNodeId(),
        type: 'text',
        x: world.x - width / 2,
        y: world.y - height / 2,
        width,
        height,
        text: '',
      }
      useCanvasStore.getState().updateDoc(relpath, (d) => applyNodeAdd(d, node))
      useCanvasStore.getState().setSelection(relpath, [node.id])
      void clientRef.current
        .patch(relpath, [{ op: 'node_add', node }])
        .catch((err) => console.warn('[nexus.canvas] node_add patch failed:', err))
    }

    canvas.addEventListener('wheel', onWheel, { passive: false })
    canvas.addEventListener('pointerdown', onPointerDown)
    canvas.addEventListener('pointermove', onPointerMove)
    canvas.addEventListener('pointerup', onPointerUp)
    canvas.addEventListener('pointercancel', onPointerUp)
    canvas.addEventListener('dblclick', onDoubleClick)
    canvas.style.cursor = 'grab'

    return () => {
      canvas.removeEventListener('wheel', onWheel)
      canvas.removeEventListener('pointerdown', onPointerDown)
      canvas.removeEventListener('pointermove', onPointerMove)
      canvas.removeEventListener('pointerup', onPointerUp)
      canvas.removeEventListener('pointercancel', onPointerUp)
      canvas.removeEventListener('dblclick', onDoubleClick)
    }
  }, [relpath])

  // ── Keyboard: delete selected ───────────────────────────────────────
  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    const onKey = (e: KeyboardEvent) => {
      // Only when the canvas container (or a child of it) has focus, so
      // typing Delete inside an unrelated pane doesn't nuke a node.
      if (!container.contains(document.activeElement) && document.activeElement !== container) {
        return
      }
      if (e.key !== 'Delete' && e.key !== 'Backspace') return
      const sel = selectionRef.current
      if (sel.size === 0) return
      e.preventDefault()
      const ids = Array.from(sel)
      useCanvasStore.getState().updateDoc(relpath, (doc) => {
        let d = doc
        for (const id of ids) d = applyNodeRemove(d, id)
        return d
      })
      useCanvasStore.getState().setSelection(relpath, [])
      const ops: CanvasPatchOp[] = ids.map((id) => ({ op: 'node_remove', id }))
      void clientRef.current
        .patch(relpath, ops)
        .catch((err) => console.warn('[nexus.canvas] node_remove patch failed:', err))
    }
    container.addEventListener('keydown', onKey)
    return () => container.removeEventListener('keydown', onKey)
  }, [relpath])

  const doc = tab?.doc
  const nodeCount = doc?.nodes.length ?? 0
  const edgeCount = doc?.edges.length ?? 0

  return (
    <div
      ref={containerRef}
      tabIndex={0}
      style={{
        position: 'relative',
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        outline: 'none',
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
