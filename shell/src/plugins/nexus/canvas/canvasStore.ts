import { create } from 'zustand'
import type { CanvasDoc, CanvasEdge, CanvasNode, CanvasPatchOp } from './kernelClient'

export interface Camera {
  /** Top-left world-space x that maps to viewport (0, 0). */
  x: number
  /** Top-left world-space y that maps to viewport (0, 0). */
  y: number
  /** 1.0 = 1:1. Clamped to [MIN_ZOOM, MAX_ZOOM] at write time. */
  zoom: number
}

export const MIN_ZOOM = 0.1
export const MAX_ZOOM = 4.0
const DEFAULT_CAMERA: Camera = { x: 0, y: 0, zoom: 1 }

function clampZoom(z: number): number {
  if (z < MIN_ZOOM) return MIN_ZOOM
  if (z > MAX_ZOOM) return MAX_ZOOM
  return z
}

/** One atomic user-visible edit. `forward` is what was applied; the
 *  canonical `inverse` op list is what must run to restore the state
 *  that existed before `forward`. Kept as op lists (not doc snapshots)
 *  so the undo pipeline drives the same `canvas_patch` path as a
 *  fresh edit — no bespoke revert codepath. */
export interface HistoryEntry {
  forward: CanvasPatchOp[]
  inverse: CanvasPatchOp[]
}

/** History cap per tab. 200 entries is plenty for a single editing
 *  session without letting a pathological drag+undo loop balloon
 *  memory. */
const HISTORY_CAP = 200

export interface CanvasTabState {
  doc: CanvasDoc | null
  loading: boolean
  error: string | null
  camera: Camera
  /** Cleared when the user has panned/zoomed at least once so we don't
   *  reset their view on every re-render. */
  cameraInitialized: boolean
  /** Set of selected node ids. Empty = no selection. */
  selection: Set<string>
  /** Id of the currently selected edge, or null. Mutually exclusive
   *  with node selection — clicking an edge clears the node set, and
   *  selecting any node clears this. Kept as a single id because the
   *  Phase-4 inspector drives one-edge-at-a-time editing and multi-
   *  edge selection isn't in scope yet. */
  selectedEdgeId: string | null
  /** Whether the 64-unit dot grid renders. Kept per-tab so toggling
   *  on one canvas doesn't flicker every other open leaf. Not
   *  serialized to the `.canvas` file — purely a view preference. */
  showGrid: boolean
  /** LIFO stack of applied edits; top is the most recent. */
  undo: HistoryEntry[]
  /** LIFO stack of undone edits available to redo; cleared whenever a
   *  non-undo/redo edit lands. */
  redo: HistoryEntry[]
}

interface CanvasStore {
  tabs: Map<string, CanvasTabState>
  setLoading: (relpath: string) => void
  setDoc: (relpath: string, doc: CanvasDoc) => void
  setError: (relpath: string, error: string) => void
  /** Surface a `canvas_patch` failure into the tab without clearing
   *  the `loading` flag (failure is on the write path, not the
   *  initial load). The doc stays optimistically applied — the user
   *  sees their edits and a banner; next flush either succeeds and
   *  clears, or fails again. WI-11 hook for the patch queue. */
  setPatchError: (relpath: string, error: string) => void
  clear: (relpath: string) => void
  get: (relpath: string) => CanvasTabState | undefined
  setCamera: (relpath: string, camera: Camera) => void
  setSelection: (relpath: string, ids: string[]) => void
  setSelectedEdge: (relpath: string, edgeId: string | null) => void
  setShowGrid: (relpath: string, showGrid: boolean) => void
  /** Apply a partial mutator to the doc. Caller is responsible for
   *  sending the matching patch to the kernel. */
  updateDoc: (relpath: string, fn: (doc: CanvasDoc) => CanvasDoc) => void
  /** Record a fresh edit; clears the redo stack. */
  pushHistory: (relpath: string, entry: HistoryEntry) => void
  /** Pop the top undo entry (if any) and move it to the redo stack. */
  popUndo: (relpath: string) => HistoryEntry | null
  /** Pop the top redo entry (if any) and move it to the undo stack. */
  popRedo: (relpath: string) => HistoryEntry | null
}

const emptyState: CanvasTabState = {
  doc: null,
  loading: false,
  error: null,
  camera: DEFAULT_CAMERA,
  cameraInitialized: false,
  selection: new Set(),
  selectedEdgeId: null,
  showGrid: true,
  undo: [],
  redo: [],
}

function produce(
  tabs: Map<string, CanvasTabState>,
  relpath: string,
  patch: Partial<CanvasTabState>,
): Map<string, CanvasTabState> {
  const next = new Map(tabs)
  const prev = next.get(relpath) ?? emptyState
  next.set(relpath, { ...prev, ...patch })
  return next
}

export const useCanvasStore = create<CanvasStore>((set, get) => ({
  tabs: new Map(),
  setLoading: (relpath) =>
    set((s) => ({ tabs: produce(s.tabs, relpath, { loading: true, error: null }) })),
  setDoc: (relpath, doc) =>
    set((s) => ({ tabs: produce(s.tabs, relpath, { doc, loading: false, error: null }) })),
  setError: (relpath, error) =>
    set((s) => ({ tabs: produce(s.tabs, relpath, { error, loading: false }) })),
  setPatchError: (relpath, error) =>
    set((s) => ({ tabs: produce(s.tabs, relpath, { error }) })),
  clear: (relpath) =>
    set((s) => {
      const next = new Map(s.tabs)
      next.delete(relpath)
      return { tabs: next }
    }),
  get: (relpath) => get().tabs.get(relpath),
  setCamera: (relpath, camera) =>
    set((s) => ({
      tabs: produce(s.tabs, relpath, {
        camera: { ...camera, zoom: clampZoom(camera.zoom) },
        cameraInitialized: true,
      }),
    })),
  setSelection: (relpath, ids) =>
    set((s) => ({
      tabs: produce(s.tabs, relpath, {
        selection: new Set(ids),
        // Selecting any node clears the edge selection so the inspector
        // switches cleanly between the two kinds of target.
        selectedEdgeId: ids.length > 0 ? null : s.tabs.get(relpath)?.selectedEdgeId ?? null,
      }),
    })),
  setSelectedEdge: (relpath, edgeId) =>
    set((s) => ({
      tabs: produce(s.tabs, relpath, {
        selectedEdgeId: edgeId,
        selection: edgeId ? new Set() : s.tabs.get(relpath)?.selection ?? new Set(),
      }),
    })),
  setShowGrid: (relpath, showGrid) =>
    set((s) => ({ tabs: produce(s.tabs, relpath, { showGrid }) })),
  updateDoc: (relpath, fn) =>
    set((s) => {
      const prev = s.tabs.get(relpath)?.doc
      if (!prev) return s
      return { tabs: produce(s.tabs, relpath, { doc: fn(prev) }) }
    }),
  pushHistory: (relpath, entry) =>
    set((s) => {
      const prev = s.tabs.get(relpath) ?? emptyState
      const undo = [...prev.undo, entry]
      if (undo.length > HISTORY_CAP) undo.splice(0, undo.length - HISTORY_CAP)
      return { tabs: produce(s.tabs, relpath, { undo, redo: [] }) }
    }),
  popUndo: (relpath) => {
    const prev = get().tabs.get(relpath)
    if (!prev || prev.undo.length === 0) return null
    const undo = prev.undo.slice(0, -1)
    const entry = prev.undo[prev.undo.length - 1]
    const redo = [...prev.redo, entry]
    set((s) => ({ tabs: produce(s.tabs, relpath, { undo, redo }) }))
    return entry
  },
  popRedo: (relpath) => {
    const prev = get().tabs.get(relpath)
    if (!prev || prev.redo.length === 0) return null
    const redo = prev.redo.slice(0, -1)
    const entry = prev.redo[prev.redo.length - 1]
    const undo = [...prev.undo, entry]
    set((s) => ({ tabs: produce(s.tabs, relpath, { undo, redo }) }))
    return entry
  },
}))

/** Apply a single CanvasPatchOp to a doc locally. Mirrors the kernel's
 *  apply_patch behaviour so the optimistic in-memory doc converges on
 *  the same state the kernel will write. */
export function applyPatchOp(doc: CanvasDoc, op: CanvasPatchOp): CanvasDoc {
  switch (op.op) {
    case 'node_add':
      if (doc.nodes.some((n) => n.id === op.node.id)) return doc
      return { ...doc, nodes: [...doc.nodes, op.node] }
    case 'node_remove':
      return {
        ...doc,
        nodes: doc.nodes.filter((n) => n.id !== op.id),
        edges: doc.edges.filter((e) => e.fromNode !== op.id && e.toNode !== op.id),
      }
    case 'node_move':
      return {
        ...doc,
        nodes: doc.nodes.map((n) => (n.id === op.id ? { ...n, x: op.x, y: op.y } : n)),
      }
    case 'node_update':
      return {
        ...doc,
        nodes: doc.nodes.map((n) => (n.id === op.node.id ? op.node : n)),
      }
    case 'edge_add':
      if (doc.edges.some((e) => e.id === op.edge.id)) return doc
      return { ...doc, edges: [...doc.edges, op.edge] }
    case 'edge_remove':
      return { ...doc, edges: doc.edges.filter((e) => e.id !== op.id) }
    case 'edge_update':
      return {
        ...doc,
        edges: doc.edges.map((e) => (e.id === op.edge.id ? op.edge : e)),
      }
    case 'set_background':
      return { ...doc, background: op.background }
  }
}

export function applyPatchOps(doc: CanvasDoc, ops: CanvasPatchOp[]): CanvasDoc {
  let d = doc
  for (const op of ops) d = applyPatchOp(d, op)
  return d
}

/** Build the inverse op list for a delete of `ids` against `doc`.
 *  Restores every node + every edge incident to any deleted node. */
export function buildDeleteInverse(
  doc: CanvasDoc,
  ids: readonly string[],
): CanvasPatchOp[] {
  const idSet = new Set(ids)
  const ops: CanvasPatchOp[] = []
  for (const n of doc.nodes) if (idSet.has(n.id)) ops.push({ op: 'node_add', node: n })
  for (const e of doc.edges) {
    if (idSet.has(e.fromNode) || idSet.has(e.toNode)) {
      ops.push({ op: 'edge_add', edge: e })
    }
  }
  return ops
}

/** Build the inverse op list for deleting a single edge — just
 *  re-adds the edge verbatim. Split out so the delete-key path in
 *  CanvasView doesn't have to care about the shape of the inverse. */
export function buildEdgeDeleteInverse(
  doc: CanvasDoc,
  edgeId: string,
): CanvasPatchOp[] {
  const edge = doc.edges.find((e) => e.id === edgeId)
  return edge ? [{ op: 'edge_add', edge }] : []
}

export type { CanvasEdge }

/** Generate a short random id for a new node. 12 chars of hex is
 *  enough for cross-session uniqueness in a single .canvas file. */
export function newNodeId(): string {
  const bytes = new Uint8Array(6)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

/** Apply a node-move op to the doc in place (creating a new doc
 *  with a single patched node; other nodes reuse their refs). */
export function applyNodeMove(doc: CanvasDoc, id: string, x: number, y: number): CanvasDoc {
  return {
    ...doc,
    nodes: doc.nodes.map((n) => (n.id === id ? { ...n, x, y } : n)),
  }
}

export function applyNodeAdd(doc: CanvasDoc, node: CanvasNode): CanvasDoc {
  return { ...doc, nodes: [...doc.nodes, node] }
}

export function applyNodeRemove(doc: CanvasDoc, id: string): CanvasDoc {
  return {
    ...doc,
    nodes: doc.nodes.filter((n) => n.id !== id),
    edges: doc.edges.filter((e) => e.fromNode !== id && e.toNode !== id),
  }
}
