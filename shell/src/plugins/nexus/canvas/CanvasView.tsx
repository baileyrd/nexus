// Phase-3 canvas surface: selection, drag-to-move, delete, and
// double-click-to-create-text. Changes flush through canvas_patch so
// the SQLite index + knowledge graph stay in sync.
//
// Deferred to a later Phase-3 cut: shift-click multi-select, marquee
// selection, resize handles, drag-from-edge create, undo/redo stack.

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  useCanvasStore,
  MIN_ZOOM,
  MAX_ZOOM,
  newNodeId,
  applyNodeAdd,
  applyNodeRemove,
  applyPatchOps,
  buildDeleteInverse,
  buildEdgeDeleteInverse,
  type Camera,
} from './canvasStore'
import {
  render,
  readTheme,
  hitTestNode,
  hitTestHandle,
  hitTestEdge,
  hitTestEdgeHandle,
  cursorForHandle,
  resizeRect,
  marqueeHit,
  marqueeFromPoints,
  DEFAULT_TEXT_NODE_SIZE,
  type MarqueeRect,
  type ResizeHandle,
  type NodeSide,
  type EdgeDragPreview,
} from './renderer'
import { Inspector } from './Inspector'
import { CanvasOverlay } from './CanvasOverlay'
import { Minimap, type MinimapHandle } from './Minimap'
import { exportCanvasPng, triggerDownload } from './exportPng'
import { autoLayout } from './autoLayout'
import { setActiveCanvas, type CanvasHandle } from './activeCanvas'
import { createPatchQueue } from './patchQueue'
import { contextKeyService } from '../../../host/ContextKeyService'
import type {
  CanvasKernelClient,
  CanvasDoc,
  CanvasEdge,
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

/** Click tolerance for edge hit-testing, in CSS pixels. Divided by
 *  camera zoom at call time so the clickable width stays constant
 *  on screen even when the user zooms out. */
const EDGE_HIT_TOLERANCE_CSS = 6

export function CanvasView({ relpath, client }: Props) {
  const tab = useCanvasStore((s) => s.tabs.get(relpath))
  const containerRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const overlayLayerRef = useRef<HTMLDivElement | null>(null)
  const minimapRef = useRef<MinimapHandle | null>(null)
  const showGridRef = useRef(true)
  // Cached refs so RAF doesn't allocate per frame and event handlers see
  // the latest values without re-binding.
  const docRef = useRef<CanvasDoc | null>(null)
  const cameraRef = useRef<Camera>({ x: 0, y: 0, zoom: 1 })
  const cameraDirtyRef = useRef(false)
  const selectionRef = useRef<Set<string>>(new Set())
  const selectedEdgeIdRef = useRef<string | null>(null)
  const marqueeRef = useRef<MarqueeRect | null>(null)
  const hoveredNodeIdRef = useRef<string | null>(null)
  const edgeDragRef = useRef<EdgeDragPreview | null>(null)
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
    selectedEdgeIdRef.current = tab?.selectedEdgeId ?? null
    showGridRef.current = tab?.showGrid ?? true
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
          selectedEdgeId: selectedEdgeIdRef.current,
          marquee: marqueeRef.current,
          hoveredNodeId: hoveredNodeIdRef.current,
          edgeDrag: edgeDragRef.current,
          showGrid: showGridRef.current,
        },
        docRef.current,
      )
      // Mirror the main render into the minimap — cheap because the
      // minimap only draws node rects + a viewport frame.
      if (minimapRef.current) {
        minimapRef.current.redraw(docRef.current, cameraRef.current, {
          w: lastWidth,
          h: lastHeight,
        })
      }
      // Mirror the camera transform onto the DOM overlay so its children
      // (world-positioned divs) line up with the 2D canvas. Written
      // imperatively so camera changes don't thrash React.
      const layer = overlayLayerRef.current
      if (layer) {
        const cam = cameraRef.current
        const tx = -cam.x * cam.zoom
        const ty = -cam.y * cam.zoom
        layer.style.transform = `translate(${tx}px, ${ty}px) scale(${cam.zoom})`
      }
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

  // WI-11 §4.3 closer: route every `canvas_patch` through a debounced
  // single-flight queue (see patchQueue.ts for the rationale). One
  // queue per canvas tab — recreated when `relpath` changes. The
  // unmount cleanup awaits a final drain so closing a tab mid-edit
  // doesn't drop pending patches.
  const queueRef = useRef<ReturnType<typeof createPatchQueue> | null>(null)
  useEffect(() => {
    const queue = createPatchQueue({
      patch: (ops) => clientRef.current.patch(relpath, ops),
      onError: (err) => {
        // Surface to the tab so the corner label flips to error
        // mode; also keep the console breadcrumb the pre-WI-11
        // handler had so existing operator runbooks still work.
        console.warn('[nexus.canvas] patch failed:', err)
        useCanvasStore.getState().setPatchError(relpath, String(err))
      },
    })
    queueRef.current = queue
    // Browser reload / tab close: best-effort flush. Async dispose
    // can't block `pagehide`, but kicking the IPC out before the
    // window dies catches the common "user typed something then
    // hit Cmd-W" path.
    const onPageHide = () => {
      void queue.flushNow()
    }
    window.addEventListener('pagehide', onPageHide)
    return () => {
      window.removeEventListener('pagehide', onPageHide)
      queueRef.current = null
      void queue.dispose()
    }
  }, [relpath])

  /** Flush a forward patch to the kernel and record the matching
   *  inverse for undo. `skipHistory = true` means the caller is itself
   *  driving undo/redo — the history stacks are managed separately in
   *  that path.
   *
   *  Pre-WI-11 this fired the IPC immediately and discarded the
   *  promise. Now it enqueues onto the per-canvas patch queue
   *  (debounced + single-flight). The `pointerup` handler calls
   *  `flushNow()` so drag-end + edge-create + resize-end always
   *  land before the next user action — preserving the existing
   *  drag-coalescing guarantee. */
  const commit = (
    forward: import('./kernelClient').CanvasPatchOp[],
    inverse: import('./kernelClient').CanvasPatchOp[],
    opts: { skipHistory?: boolean } = {},
  ) => {
    if (forward.length === 0) return
    if (!opts.skipHistory) {
      useCanvasStore.getState().pushHistory(relpath, { forward, inverse })
    }
    queueRef.current?.enqueue(forward)
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
      | {
          kind: 'create-edge'
          fromNodeId: string
          fromSide: NodeSide
        }
    let drag: DragMode = { kind: 'none' }

    /** Hover helper: when exactly one non-group node is selected and
     *  the pointer is over one of its resize handles, show the right
     *  cursor. Also tracks the hovered node so the edge-create
     *  affordances render. */
    const updateHoverCursor = (cx: number, cy: number) => {
      const doc = docRef.current
      const cam = cameraRef.current
      const world = screenToWorld(cx, cy)

      // Hovered-for-edge-create: topmost non-group node under the
      // cursor, or within the affordance offset area. We also promote
      // to "hovered" when the cursor is over one of the 4 affordance
      // circles (which sit outside the node rect).
      let hovered: string | null = null
      if (doc) {
        const over = hitTestNode(doc.nodes, world.x, world.y)
        if (over && over.type !== 'group') {
          hovered = over.id
        } else {
          for (const n of doc.nodes) {
            if (n.type === 'group') continue
            if (hitTestEdgeHandle(n, cam, cx, cy)) {
              hovered = n.id
              break
            }
          }
        }
      }
      if (hovered !== hoveredNodeIdRef.current) {
        hoveredNodeIdRef.current = hovered
      }

      // Cursor priority: resize handle > edge-create affordance > grab.
      const sel = selectionRef.current
      if (sel.size === 1 && doc) {
        const only = doc.nodes.find((n) => n.id === Array.from(sel)[0])
        if (only && only.type !== 'group') {
          const handle = hitTestHandle(only, cam, cx, cy)
          if (handle) {
            canvas.style.cursor = cursorForHandle(handle)
            return
          }
        }
      }
      if (hovered) {
        const n = doc?.nodes.find((x) => x.id === hovered)
        if (n && hitTestEdgeHandle(n, cam, cx, cy)) {
          canvas.style.cursor = 'crosshair'
          return
        }
      }
      canvas.style.cursor = 'grab'
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

      // Edge-create affordances take priority: they sit outside the
      // node rect so a pointerdown there would otherwise route to
      // empty-space (marquee). Check every non-group node — the
      // affordance area is small and affordance-on-node-A doesn't
      // overlap affordance-on-node-B in practice.
      if (doc) {
        for (const n of doc.nodes) {
          if (n.type === 'group') continue
          const side = hitTestEdgeHandle(n, cameraRef.current, cx, cy)
          if (side) {
            drag = { kind: 'create-edge', fromNodeId: n.id, fromSide: side }
            edgeDragRef.current = {
              fromNodeId: n.id,
              fromSide: side,
              toWorld: world,
            }
            canvas.setPointerCapture(e.pointerId)
            canvas.style.cursor = 'crosshair'
            return
          }
        }
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
        // Before falling through to a marquee, give edges a shot at the
        // click — users expect to click a thin curve to select it.
        // Tolerance shrinks with zoom so the clickable band stays the
        // same width on screen.
        if (doc) {
          const tol = EDGE_HIT_TOLERANCE_CSS / cameraRef.current.zoom
          const edgeId = hitTestEdge(doc, world.x, world.y, tol)
          if (edgeId) {
            useCanvasStore.getState().setSelectedEdge(relpath, edgeId)
            // Don't start a drag — a plain click is select-only. A
            // subsequent drag (if the user keeps moving) would just
            // start a marquee, which is fine.
            return
          }
        }
        // Empty-space left-drag → marquee. Non-additive clicks clear
        // the selection on down (feels snappier than waiting for up).
        const base = additive ? new Set(selectionRef.current) : new Set<string>()
        if (!additive) {
          useCanvasStore.getState().setSelection(relpath, [])
          useCanvasStore.getState().setSelectedEdge(relpath, null)
        }
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
      if (drag.kind === 'create-edge') {
        const rect = canvas.getBoundingClientRect()
        const cx = e.clientX - rect.left
        const cy = e.clientY - rect.top
        const world = screenToWorld(cx, cy)
        edgeDragRef.current = {
          fromNodeId: drag.fromNodeId,
          fromSide: drag.fromSide,
          toWorld: world,
        }
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
      // Drag-end flush: any patch enqueued by the gesture-end
      // commits below should land before the next user action, so
      // the existing structural drag-coalescing guarantee survives
      // the move to a debounced queue (WI-11). Wrap the dispatch
      // so every early-return path still triggers the flush.
      try {
        dispatchPointerUp(finished)
      } finally {
        void queueRef.current?.flushNow()
      }
    }

    const dispatchPointerUp = (finished: Exclude<DragMode, { kind: 'none' }>) => {
      if (finished.kind === 'marquee') {
        marqueeRef.current = null
        return
      }

      if (finished.kind === 'create-edge') {
        const preview = edgeDragRef.current
        edgeDragRef.current = null
        if (!preview) return
        const doc = docRef.current
        if (!doc) return
        const world = preview.toWorld

        // Release over another non-group node → just an edge.
        // Release over empty space → new text node + edge.
        // Release over the source node → cancel.
        const target = hitTestNode(doc.nodes, world.x, world.y)
        const forward: CanvasPatchOp[] = []
        const inverse: CanvasPatchOp[] = []
        if (target && target.id === finished.fromNodeId) {
          return
        }
        const edgeId = newNodeId()
        let toNodeId: string
        if (target && target.type !== 'group') {
          toNodeId = target.id
        } else {
          const { width, height } = DEFAULT_TEXT_NODE_SIZE
          const newNode: CanvasNode = {
            id: newNodeId(),
            type: 'text',
            x: world.x - width / 2,
            y: world.y - height / 2,
            width,
            height,
            text: '',
          }
          toNodeId = newNode.id
          forward.push({ op: 'node_add', node: newNode })
          inverse.push({ op: 'node_remove', id: newNode.id })
          useCanvasStore.getState().updateDoc(relpath, (d) => applyNodeAdd(d, newNode))
        }
        const edge = {
          id: edgeId,
          fromNode: finished.fromNodeId,
          toNode: toNodeId,
        }
        forward.push({ op: 'edge_add', edge })
        // Inverse prepended so undo drops the edge before the node it
        // pointed to, matching apply_patch's serial semantics.
        inverse.unshift({ op: 'edge_remove', id: edgeId })
        useCanvasStore.getState().updateDoc(relpath, (d) => ({
          ...d,
          edges: [...d.edges, edge],
        }))
        useCanvasStore.getState().setSelection(relpath, [toNodeId])
        commitRef.current(forward, inverse)
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

  // Keyboard actions are routed through the keybinding plugin via
  // commands registered in this plugin's `activate`. The container's
  // focus state flips the `canvas.focused` context key so the
  // chords only fire when a canvas actually owns focus.
  //
  // Handlers shared between the keybinding commands and the control-
  // strip buttons live in the closures below; the CanvasHandle
  // published to `setActiveCanvas` just proxies them.
  const applyHistoryEntry = useCallback((ops: CanvasPatchOp[]) => {
    if (ops.length === 0) return
    useCanvasStore.getState().updateDoc(relpath, (d) => applyPatchOps(d, ops))
    // Route undo/redo through the same queue so a rapid Ctrl+Z mash
    // collapses into one IPC and never reorders against an in-flight
    // direct edit (single-flight serialises both code paths). Flush
    // immediately — the user is asking for a state change "now".
    queueRef.current?.enqueue(ops)
    void queueRef.current?.flushNow()
  }, [relpath])

  const runUndo = useCallback(() => {
    const entry = useCanvasStore.getState().popUndo(relpath)
    if (!entry) return
    applyHistoryEntry(entry.inverse)
  }, [relpath, applyHistoryEntry])

  const runRedo = useCallback(() => {
    const entry = useCanvasStore.getState().popRedo(relpath)
    if (!entry) return
    applyHistoryEntry(entry.forward)
  }, [relpath, applyHistoryEntry])

  const runDelete = useCallback(() => {
    const doc = docRef.current
    if (!doc) return
    const selectedEdgeId = selectedEdgeIdRef.current
    if (selectedEdgeId) {
      const inverse = buildEdgeDeleteInverse(doc, selectedEdgeId)
      if (inverse.length === 0) return
      useCanvasStore.getState().updateDoc(relpath, (d) => ({
        ...d,
        edges: d.edges.filter((edge) => edge.id !== selectedEdgeId),
      }))
      useCanvasStore.getState().setSelectedEdge(relpath, null)
      commitRef.current([{ op: 'edge_remove', id: selectedEdgeId }], inverse)
      return
    }
    const sel = selectionRef.current
    if (sel.size === 0) return
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
  }, [relpath])

  const runFit = useCallback((selectionOnly: boolean) => {
    const doc = docRef.current
    const canvas = canvasRef.current
    if (!doc || !canvas) return
    const rect = canvas.getBoundingClientRect()
    const sel = selectionRef.current
    const targets =
      selectionOnly && sel.size > 0 ? doc.nodes.filter((n) => sel.has(n.id)) : doc.nodes
    if (targets.length === 0) return
    cameraRef.current = fitCameraToNodes(targets, rect.width, rect.height)
    cameraDirtyRef.current = true
  }, [])

  const doc = tab?.doc
  const nodeCount = doc?.nodes.length ?? 0
  const edgeCount = doc?.edges.length ?? 0
  const showGrid = tab?.showGrid ?? true
  const [exporting, setExporting] = useState(false)
  const [showHelp, setShowHelp] = useState(false)
  const [showDocInspector, setShowDocInspector] = useState(false)

  // Minimap click-drag: recenter the camera so the clicked world
  // point sits at the middle of the main viewport. Stable callback
  // so Minimap's pointerdown effect doesn't reattach every frame.
  const onMinimapRecenter = useCallback(
    (worldX: number, worldY: number) => {
      const canvas = canvasRef.current
      if (!canvas) return
      const rect = canvas.getBoundingClientRect()
      const cam = cameraRef.current
      cameraRef.current = {
        x: worldX - rect.width / (2 * cam.zoom),
        y: worldY - rect.height / (2 * cam.zoom),
        zoom: cam.zoom,
      }
      cameraDirtyRef.current = true
    },
    [],
  )

  const onToggleGrid = () => {
    useCanvasStore.getState().setShowGrid(relpath, !showGrid)
  }

  const onTidy = () => {
    const currentDoc = docRef.current
    if (!currentDoc) return
    const moves = autoLayout(currentDoc)
    if (moves.length === 0) return
    // Build forward + inverse node_move pairs straight from the
    // pre-layout doc so undo restores every position verbatim.
    const byId = new Map(currentDoc.nodes.map((n) => [n.id, n]))
    const forward: CanvasPatchOp[] = []
    const inverse: CanvasPatchOp[] = []
    for (const m of moves) {
      const before = byId.get(m.id)
      if (!before) continue
      forward.push({ op: 'node_move', id: m.id, x: m.x, y: m.y })
      inverse.push({ op: 'node_move', id: m.id, x: before.x, y: before.y })
    }
    if (forward.length === 0) return
    useCanvasStore.getState().updateDoc(relpath, (d) => ({
      ...d,
      nodes: d.nodes.map((n) => {
        const mv = moves.find((m) => m.id === n.id)
        return mv ? { ...n, x: mv.x, y: mv.y } : n
      }),
    }))
    commitRef.current(forward, inverse)
  }

  const onExportPng = async () => {
    const container = containerRef.current
    const currentDoc = docRef.current
    if (!container || !currentDoc || exporting) return
    setExporting(true)
    try {
      const blob = await exportCanvasPng(currentDoc, container)
      if (!blob) return
      const base = basenameNoExt(relpath) || 'canvas'
      triggerDownload(blob, `${base}.png`)
    } finally {
      setExporting(false)
    }
  }

  // Resolve the currently-inspected target. Edge selection wins over
  // node selection (they're mutually exclusive in the store, but be
  // defensive here). For nodes, the inspector only binds when exactly
  // one is selected — multi-node property editing is out of scope for
  // Phase 4.
  const selectedEdge: CanvasEdge | null = tab?.selectedEdgeId && doc
    ? doc.edges.find((e) => e.id === tab.selectedEdgeId) ?? null
    : null
  const selectedNode: CanvasNode | null =
    !selectedEdge && tab?.selection && tab.selection.size === 1 && doc
      ? doc.nodes.find((n) => n.id === Array.from(tab.selection)[0]) ?? null
      : null

  const onUpdateNode = (next: CanvasNode, prev: CanvasNode) => {
    useCanvasStore.getState().updateDoc(relpath, (d) => ({
      ...d,
      nodes: d.nodes.map((n) => (n.id === next.id ? next : n)),
    }))
    commitRef.current(
      [{ op: 'node_update', node: next }],
      [{ op: 'node_update', node: prev }],
    )
  }
  const onUpdateEdge = (next: CanvasEdge, prev: CanvasEdge) => {
    useCanvasStore.getState().updateDoc(relpath, (d) => ({
      ...d,
      edges: d.edges.map((e) => (e.id === next.id ? next : e)),
    }))
    commitRef.current(
      [{ op: 'edge_update', edge: next }],
      [{ op: 'edge_update', edge: prev }],
    )
  }
  const onUpdateBackground = (
    next: import('./kernelClient').CanvasBackground | null,
    prev: import('./kernelClient').CanvasBackground | null,
  ) => {
    useCanvasStore.getState().updateDoc(relpath, (d) => ({ ...d, background: next }))
    commitRef.current(
      [{ op: 'set_background', background: next }],
      [{ op: 'set_background', background: prev }],
    )
  }

  const onExportSvg = async () => {
    const container = containerRef.current
    const currentDoc = docRef.current
    if (!container || !currentDoc || exporting) return
    setExporting(true)
    try {
      const { exportCanvasSvg } = await import('./exportFormats')
      const blob = await exportCanvasSvg(currentDoc, container)
      if (!blob) return
      const base = basenameNoExt(relpath) || 'canvas'
      triggerDownload(blob, `${base}.svg`)
    } finally {
      setExporting(false)
    }
  }

  const onExportPdf = async () => {
    const container = containerRef.current
    const currentDoc = docRef.current
    if (!container || !currentDoc || exporting) return
    setExporting(true)
    try {
      const { exportCanvasPdf } = await import('./exportFormats')
      const blob = await exportCanvasPdf(currentDoc, container)
      if (!blob) return
      const base = basenameNoExt(relpath) || 'canvas'
      triggerDownload(blob, `${base}.pdf`)
    } finally {
      setExporting(false)
    }
  }

  // Publish the active-canvas handle whenever focus lands inside the
  // container. Commands registered by the plugin dispatch against
  // whichever canvas last claimed focus.
  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    const handle: CanvasHandle = {
      undo: runUndo,
      redo: runRedo,
      deleteSelected: runDelete,
      fit: () => runFit(false),
      fitSelection: () => runFit(true),
      toggleHelp: () => setShowHelp((v) => !v),
      closeHelp: () => setShowHelp(false),
      toggleGrid: onToggleGrid,
      toggleBackgroundInspector: () => setShowDocInspector((v) => !v),
      tidy: onTidy,
      exportPng: onExportPng,
      exportSvg: onExportSvg,
      exportPdf: onExportPdf,
    }
    const claimFocus = () => {
      setActiveCanvas(handle)
      contextKeyService.set('canvas.focused', true)
    }
    const releaseFocus = () => {
      setActiveCanvas(null)
      contextKeyService.set('canvas.focused', false)
    }
    // Ensure clicking anywhere in the canvas container focuses it,
    // so the global keybinding dispatcher's `canvas.focused` gate
    // opens as soon as the user interacts. Without this, the div
    // (tabIndex=0) only gets focus via keyboard navigation.
    const focusOnPointer = () => {
      if (document.activeElement !== container && !container.contains(document.activeElement)) {
        container.focus({ preventScroll: true })
      }
    }
    container.addEventListener('focusin', claimFocus)
    const onFocusOut = (e: FocusEvent) => {
      const next = e.relatedTarget as Node | null
      if (next && container.contains(next)) return
      releaseFocus()
    }
    container.addEventListener('focusout', onFocusOut)
    container.addEventListener('pointerdown', focusOnPointer)
    return () => {
      releaseFocus()
      container.removeEventListener('focusin', claimFocus)
      container.removeEventListener('focusout', onFocusOut)
      container.removeEventListener('pointerdown', focusOnPointer)
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runUndo, runRedo, runDelete, runFit, onToggleGrid, onTidy, onExportPng])

  // Keep the help-overlay context key in sync so the Escape binding
  // only fires when the overlay is actually open.
  useEffect(() => {
    contextKeyService.set('canvas.helpOpen', showHelp)
  }, [showHelp])

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
      <CanvasOverlay ref={overlayLayerRef} nodes={doc?.nodes ?? []} client={client} />
      {tab?.loading && <CornerLabel>Loading…</CornerLabel>}
      {tab?.error && <CornerLabel>Error: {tab.error}</CornerLabel>}
      {!tab?.loading && !tab?.error && (
        <CornerLabel>
          {nodeCount} node{nodeCount === 1 ? '' : 's'} · {edgeCount} edge
          {edgeCount === 1 ? '' : 's'} · {Math.round((tab?.camera.zoom ?? 1) * 100)}%
        </CornerLabel>
      )}
      {(selectedNode || selectedEdge || showDocInspector) && (
        <Inspector
          node={selectedNode}
          edge={selectedEdge}
          docBackground={doc?.background ?? null}
          showDocInspector={showDocInspector && !selectedNode && !selectedEdge}
          onUpdateNode={onUpdateNode}
          onUpdateEdge={onUpdateEdge}
          onUpdateBackground={onUpdateBackground}
          onCloseDocInspector={() => setShowDocInspector(false)}
        />
      )}
      <Minimap ref={minimapRef} onRecenter={onMinimapRecenter} />
      <ControlStrip
        showGrid={showGrid}
        onToggleGrid={onToggleGrid}
        onTidy={onTidy}
        onExportPng={onExportPng}
        onExportSvg={onExportSvg}
        onExportPdf={onExportPdf}
        exporting={exporting}
        canExport={nodeCount > 0}
        canTidy={nodeCount > 1}
        onShowHelp={() => setShowHelp(true)}
        docInspectorOpen={showDocInspector}
        onToggleDocInspector={() => setShowDocInspector((v) => !v)}
      />
      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}
    </div>
  )
}

function ControlStrip({
  showGrid,
  onToggleGrid,
  onTidy,
  onExportPng,
  onExportSvg,
  onExportPdf,
  exporting,
  canExport,
  canTidy,
  onShowHelp,
  docInspectorOpen,
  onToggleDocInspector,
}: {
  showGrid: boolean
  onToggleGrid: () => void
  onTidy: () => void
  onExportPng: () => void
  onExportSvg: () => void
  onExportPdf: () => void
  exporting: boolean
  canExport: boolean
  canTidy: boolean
  onShowHelp: () => void
  docInspectorOpen: boolean
  onToggleDocInspector: () => void
}) {
  const [exportOpen, setExportOpen] = useState(false)
  return (
    <div
      data-canvas-export-exclude="true"
      style={{
        position: 'absolute',
        left: 12,
        bottom: 12,
        display: 'flex',
        gap: 6,
        padding: 4,
        borderRadius: 6,
        background: 'var(--bg-raised, #2d2d2d)',
        border: '1px solid var(--divider-color, #3f3f46)',
        boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
        pointerEvents: 'auto',
      }}
    >
      <ControlButton
        active={showGrid}
        onClick={onToggleGrid}
        title={showGrid ? 'Hide grid' : 'Show grid'}
      >
        Grid
      </ControlButton>
      <ControlButton
        onClick={onTidy}
        disabled={!canTidy}
        title={canTidy ? 'Auto-layout nodes' : 'Need at least two nodes'}
      >
        Tidy
      </ControlButton>
      <div style={{ position: 'relative' }}>
        <ControlButton
          onClick={() => setExportOpen((v) => !v)}
          disabled={!canExport || exporting}
          active={exportOpen}
          title={canExport ? 'Export canvas (PNG / SVG / PDF)' : 'Nothing to export'}
        >
          {exporting ? '…' : 'Export'}
        </ControlButton>
        {exportOpen && (
          <div
            style={{
              position: 'absolute',
              left: 0,
              bottom: '110%',
              display: 'flex',
              flexDirection: 'column',
              background: 'var(--bg-raised, #2d2d2d)',
              border: '1px solid var(--divider-color, #3f3f46)',
              borderRadius: 4,
              boxShadow: '0 4px 12px rgba(0,0,0,0.4)',
              minWidth: 90,
              padding: 2,
            }}
          >
            {(
              [
                { label: 'PNG', run: onExportPng },
                { label: 'SVG', run: onExportSvg },
                { label: 'PDF', run: onExportPdf },
              ] as const
            ).map(({ label, run }) => (
              <button
                key={label}
                type="button"
                onClick={() => {
                  setExportOpen(false)
                  run()
                }}
                style={{
                  padding: '4px 10px',
                  background: 'transparent',
                  color: 'var(--fg, #e5e7eb)',
                  border: 'none',
                  textAlign: 'left',
                  cursor: 'pointer',
                  fontSize: 11,
                  fontFamily: 'inherit',
                }}
              >
                {label}
              </button>
            ))}
          </div>
        )}
      </div>
      <ControlButton
        active={docInspectorOpen}
        onClick={onToggleDocInspector}
        title="Canvas background"
      >
        BG
      </ControlButton>
      <ControlButton onClick={onShowHelp} title="Keyboard shortcuts (?)">
        ?
      </ControlButton>
    </div>
  )
}

function HelpOverlay({ onClose }: { onClose: () => void }) {
  // HelpOverlay is chrome — exclude from export snapshots.
  const rows: Array<[string, string]> = [
    ['Click', 'Select node'],
    ['Shift + click', 'Toggle in selection'],
    ['Drag empty space', 'Marquee select'],
    ['Drag node', 'Move node(s)'],
    ['Drag node border handle', 'Create edge (or new node + edge)'],
    ['Double-click empty space', 'New text node'],
    ['Middle-click + drag', 'Pan canvas'],
    ['Wheel', 'Scroll / pan'],
    ['Ctrl / Cmd + wheel', 'Zoom'],
    ['f', 'Zoom to fit content'],
    ['Shift + f', 'Zoom to selection'],
    ['Delete / Backspace', 'Delete selected node or edge'],
    ['Ctrl / Cmd + Z', 'Undo'],
    ['Ctrl / Cmd + Shift + Z', 'Redo'],
    ['?', 'Toggle this help'],
    ['Escape', 'Close help'],
    ['—', 'Command palette → type "Canvas" for all commands (export PNG / SVG / PDF, background, tidy, grid toggle)'],
  ]
  return (
    <div
      onClick={onClose}
      data-canvas-export-exclude="true"
      style={{
        position: 'absolute',
        inset: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'rgba(0,0,0,0.45)',
        zIndex: 10,
        // Overlay root is interactive — clicking the backdrop
        // dismisses, clicking the card is a no-op via stopPropagation.
        pointerEvents: 'auto',
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          minWidth: 360,
          maxWidth: 440,
          padding: 18,
          borderRadius: 8,
          background: 'var(--bg-raised, #2d2d2d)',
          border: '1px solid var(--divider-color, #3f3f46)',
          color: 'var(--fg, #e5e7eb)',
          fontFamily: 'var(--font-family, system-ui, sans-serif)',
          fontSize: 12,
          boxShadow: '0 8px 28px rgba(0,0,0,0.45)',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            marginBottom: 12,
          }}
        >
          <div
            style={{
              fontSize: 13,
              fontWeight: 600,
              letterSpacing: 0.3,
            }}
          >
            Canvas shortcuts
          </div>
          <button
            type="button"
            onClick={onClose}
            style={{
              background: 'transparent',
              border: 'none',
              color: 'var(--fg-muted, #9ca3af)',
              fontSize: 16,
              cursor: 'pointer',
              padding: 0,
              lineHeight: 1,
            }}
            aria-label="Close"
          >
            ×
          </button>
        </div>
        <table style={{ width: '100%', borderCollapse: 'collapse' }}>
          <tbody>
            {rows.map(([key, desc]) => (
              <tr key={key}>
                <td
                  style={{
                    padding: '4px 10px 4px 0',
                    color: 'var(--fg-muted, #9ca3af)',
                    fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
                    fontSize: 11,
                    whiteSpace: 'nowrap',
                    verticalAlign: 'top',
                  }}
                >
                  {key}
                </td>
                <td style={{ padding: '4px 0', verticalAlign: 'top' }}>{desc}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function ControlButton({
  active,
  onClick,
  children,
  title,
  disabled,
}: {
  active?: boolean
  onClick: () => void
  children: React.ReactNode
  title: string
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      disabled={disabled}
      style={{
        background: active
          ? 'var(--accent, #3b82f6)'
          : 'var(--bg-muted, #1e1e1e)',
        color: active ? '#fff' : 'var(--fg, #e5e7eb)',
        border: '1px solid var(--divider-color, #3f3f46)',
        borderRadius: 4,
        padding: '4px 10px',
        fontSize: 11,
        fontWeight: 500,
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.4 : 1,
        fontFamily: 'inherit',
      }}
    >
      {children}
    </button>
  )
}

function basenameNoExt(path: string): string {
  if (!path) return ''
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  const base = i >= 0 ? path.slice(i + 1) : path
  const dot = base.lastIndexOf('.')
  return dot > 0 ? base.slice(0, dot) : base
}

function CornerLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      data-canvas-export-exclude="true"
      style={{
        position: 'absolute',
        bottom: 8,
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
  return fitCameraToNodes(doc.nodes, viewW, viewH)
}

/** Shared fit-camera helper. Used for initial zoom-to-fit, the 'f'
 *  shortcut (fit to all content), and 'Shift+f' (fit to selection). */
function fitCameraToNodes(
  nodes: readonly CanvasNode[],
  viewW: number,
  viewH: number,
): Camera {
  if (nodes.length === 0) return { x: -viewW / 2, y: -viewH / 2, zoom: 1 }
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const n of nodes) {
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
