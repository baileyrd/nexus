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
  applyNodeAdd,
  applyNodeRemove,
  applyPatchOps,
  buildDeleteInverse,
  type Camera,
} from './canvasStore'
import {
  render,
  readTheme,
  hitTestNode,
  hitTestHandle,
  cursorForHandle,
  resizeRect,
  marqueeHit,
  marqueeFromPoints,
  DEFAULT_TEXT_NODE_SIZE,
  type MarqueeRect,
  type ResizeHandle,
} from './renderer'
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
  const marqueeRef = useRef<MarqueeRect | null>(null)
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
          marquee: marqueeRef.current,
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

  // ── Commit helpers ───────────────────────────────────────────────────

  /** Flush a forward patch to the kernel and record the matching
   *  inverse for undo. `skipHistory = true` means the caller is itself
   *  driving undo/redo — the history stacks are managed separately in
   *  that path. */
  const commit = (
    forward: import('./kernelClient').CanvasPatchOp[],
    inverse: import('./kernelClient').CanvasPatchOp[],
    opts: { skipHistory?: boolean } = {},
  ) => {
    if (forward.length === 0) return
    if (!opts.skipHistory) {
      useCanvasStore.getState().pushHistory(relpath, { forward, inverse })
    }
    void clientRef.current
      .patch(relpath, forward)
      .catch((err) => console.warn('[nexus.canvas] patch failed:', err))
  }

  const commitRef = useRef(commit)
  commitRef.current = commit

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
          startPositions: Map<string, { x: number; y: number }>
          armed: boolean
          downCX: number
          downCY: number
        }
      | {
          kind: 'marquee'
          /** World-space anchor of the drag. */
          startWorld: { x: number; y: number }
          /** Selection before the drag — used as the base for shift-
           *  additive marquees so previously-selected nodes stay lit. */
          base: Set<string>
          additive: boolean
        }
      | {
          kind: 'resize-node'
          nodeId: string
          handle: ResizeHandle
          startWorldX: number
          startWorldY: number
          startRect: { x: number; y: number; width: number; height: number }
        }
    let drag: DragMode = { kind: 'none' }

    /** Hover helper: when exactly one non-group node is selected and
     *  the pointer is over one of its resize handles, show the right
     *  cursor. Called from pointermove when not mid-drag. */
    const updateHoverCursor = (cx: number, cy: number) => {
      const sel = selectionRef.current
      if (sel.size !== 1) {
        canvas.style.cursor = 'grab'
        return
      }
      const doc = docRef.current
      if (!doc) return
      const only = doc.nodes.find((n) => n.id === Array.from(sel)[0])
      if (!only || only.type === 'group') {
        canvas.style.cursor = 'grab'
        return
      }
      const handle = hitTestHandle(only, cameraRef.current, cx, cy)
      canvas.style.cursor = handle ? cursorForHandle(handle) : 'grab'
    }

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0 && e.button !== 1) return
      const rect = canvas.getBoundingClientRect()
      const cx = e.clientX - rect.left
      const cy = e.clientY - rect.top
      const world = screenToWorld(cx, cy)
      const doc = docRef.current
      const hit = doc ? hitTestNode(doc.nodes, world.x, world.y) : null
      const additive = e.shiftKey

      // Middle click → pan regardless of what's under the cursor.
      if (e.button === 1) {
        drag = { kind: 'pan', lastX: e.clientX, lastY: e.clientY }
        canvas.setPointerCapture(e.pointerId)
        canvas.style.cursor = 'grabbing'
        return
      }

      // Resize handles take priority over everything else when a single
      // non-group node is selected — they sit on top of the node so we
      // have to check them before the hit-test falls through to
      // move-drag semantics.
      const sel = selectionRef.current
      if (sel.size === 1 && doc) {
        const only = doc.nodes.find((n) => n.id === Array.from(sel)[0])
        if (only && only.type !== 'group') {
          const handle = hitTestHandle(only, cameraRef.current, cx, cy)
          if (handle) {
            drag = {
              kind: 'resize-node',
              nodeId: only.id,
              handle,
              startWorldX: world.x,
              startWorldY: world.y,
              startRect: {
                x: only.x,
                y: only.y,
                width: only.width,
                height: only.height,
              },
            }
            canvas.setPointerCapture(e.pointerId)
            canvas.style.cursor = cursorForHandle(handle)
            return
          }
        }
      }

      if (!hit) {
        // Empty-space left-drag → marquee. Non-additive clicks clear
        // the selection on down (feels snappier than waiting for up).
        const base = additive ? new Set(selectionRef.current) : new Set<string>()
        if (!additive) useCanvasStore.getState().setSelection(relpath, [])
        drag = { kind: 'marquee', startWorld: world, base, additive }
        marqueeRef.current = { x: world.x, y: world.y, width: 0, height: 0 }
        canvas.setPointerCapture(e.pointerId)
        return
      }

      // Node click: shift toggles, plain click replaces.
      const curr = selectionRef.current
      let nextSel: string[]
      if (additive) {
        const next = new Set(curr)
        if (next.has(hit.id)) next.delete(hit.id)
        else next.add(hit.id)
        nextSel = Array.from(next)
      } else {
        nextSel = curr.has(hit.id) ? Array.from(curr) : [hit.id]
      }
      useCanvasStore.getState().setSelection(relpath, nextSel)

      // Arm a move drag for every selected node — moves a multi-select
      // as a rigid group.
      const selectedSet = new Set(nextSel)
      const startPositions = new Map<string, { x: number; y: number }>()
      if (doc) {
        for (const n of doc.nodes) {
          if (selectedSet.has(n.id)) startPositions.set(n.id, { x: n.x, y: n.y })
        }
      }
      drag = {
        kind: 'move-node',
        nodeId: hit.id,
        startWorldX: world.x,
        startWorldY: world.y,
        startPositions,
        armed: false,
        downCX: cx,
        downCY: cy,
      }
      canvas.setPointerCapture(e.pointerId)
    }

    const onPointerMove = (e: PointerEvent) => {
      if (drag.kind === 'none') {
        const rect = canvas.getBoundingClientRect()
        updateHoverCursor(e.clientX - rect.left, e.clientY - rect.top)
        return
      }
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
      if (drag.kind === 'resize-node') {
        const rect = canvas.getBoundingClientRect()
        const cx = e.clientX - rect.left
        const cy = e.clientY - rect.top
        const world = screenToWorld(cx, cy)
        const dx = world.x - drag.startWorldX
        const dy = world.y - drag.startWorldY
        const next = resizeRect(drag.startRect, drag.handle, dx, dy, e.shiftKey)
        const nodeId = drag.nodeId
        useCanvasStore.getState().updateDoc(relpath, (doc) => ({
          ...doc,
          nodes: doc.nodes.map((n) =>
            n.id === nodeId ? { ...n, ...next } : n,
          ),
        }))
        return
      }
      if (drag.kind === 'marquee') {
        const rect = canvas.getBoundingClientRect()
        const cx = e.clientX - rect.left
        const cy = e.clientY - rect.top
        const world = screenToWorld(cx, cy)
        const marquee = marqueeFromPoints(drag.startWorld, world)
        marqueeRef.current = marquee
        const doc = docRef.current
        if (doc) {
          const hits = marqueeHit(doc.nodes, marquee)
          const next = new Set(drag.base)
          for (const id of hits) next.add(id)
          // Only write when the selection actually changes — skip
          // redundant store writes during fine marquee movement.
          if (!setEquals(selectionRef.current, next)) {
            useCanvasStore.getState().setSelection(relpath, Array.from(next))
          }
        }
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
      const dx = world.x - drag.startWorldX
      const dy = world.y - drag.startWorldY
      const starts = drag.startPositions
      useCanvasStore.getState().updateDoc(relpath, (doc) => ({
        ...doc,
        nodes: doc.nodes.map((n) => {
          const start = starts.get(n.id)
          return start ? { ...n, x: start.x + dx, y: start.y + dy } : n
        }),
      }))
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

      if (finished.kind === 'marquee') {
        marqueeRef.current = null
        return
      }

      if (finished.kind === 'resize-node') {
        const doc = docRef.current
        if (!doc) return
        const node = doc.nodes.find((n) => n.id === finished.nodeId)
        if (!node) return
        const s = finished.startRect
        if (
          node.x === s.x &&
          node.y === s.y &&
          node.width === s.width &&
          node.height === s.height
        ) {
          return
        }
        // Inverse restores every field on the node — use the pre-drag
        // snapshot so undo also reverses type-specific edits if the
        // resize shape ever grows beyond x/y/w/h.
        const before: CanvasNode = { ...node, ...s }
        commitRef.current(
          [{ op: 'node_update', node }],
          [{ op: 'node_update', node: before }],
        )
        return
      }

      if (finished.kind === 'move-node' && finished.armed) {
        const doc = docRef.current
        if (!doc) return
        const forward: CanvasPatchOp[] = []
        const inverse: CanvasPatchOp[] = []
        for (const n of doc.nodes) {
          const start = finished.startPositions.get(n.id)
          if (!start) continue
          if (n.x === start.x && n.y === start.y) continue
          forward.push({ op: 'node_move', id: n.id, x: n.x, y: n.y })
          inverse.push({ op: 'node_move', id: n.id, x: start.x, y: start.y })
        }
        if (forward.length === 0) return
        commitRef.current(forward, inverse)
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
      commitRef.current(
        [{ op: 'node_add', node }],
        [{ op: 'node_remove', id: node.id }],
      )
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
      const isUndo = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && !e.shiftKey
      const isRedo =
        ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && e.shiftKey) ||
        ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'y')

      if (isUndo) {
        e.preventDefault()
        const entry = useCanvasStore.getState().popUndo(relpath)
        if (!entry) return
        applyHistoryEntry(entry.inverse)
        return
      }
      if (isRedo) {
        e.preventDefault()
        const entry = useCanvasStore.getState().popRedo(relpath)
        if (!entry) return
        applyHistoryEntry(entry.forward)
        return
      }

      if (e.key !== 'Delete' && e.key !== 'Backspace') return
      const sel = selectionRef.current
      if (sel.size === 0) return
      e.preventDefault()
      const doc = docRef.current
      if (!doc) return
      const ids = Array.from(sel)
      const inverse = buildDeleteInverse(doc, ids)
      useCanvasStore.getState().updateDoc(relpath, (d) => {
        let out = d
        for (const id of ids) out = applyNodeRemove(out, id)
        return out
      })
      useCanvasStore.getState().setSelection(relpath, [])
      const forward: CanvasPatchOp[] = ids.map((id) => ({ op: 'node_remove', id }))
      commitRef.current(forward, inverse)
    }

    /** Drive undo/redo: apply `ops` locally and flush to the kernel.
     *  Skips the history push because popUndo/popRedo already moved
     *  the entry between stacks. */
    const applyHistoryEntry = (ops: CanvasPatchOp[]) => {
      if (ops.length === 0) return
      useCanvasStore.getState().updateDoc(relpath, (d) => applyPatchOps(d, ops))
      void clientRef.current
        .patch(relpath, ops)
        .catch((err) => console.warn('[nexus.canvas] undo/redo patch failed:', err))
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

function setEquals(a: Set<string>, b: Set<string>): boolean {
  if (a.size !== b.size) return false
  for (const v of a) if (!b.has(v)) return false
  return true
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
