import { create } from 'zustand'
import type { CanvasDoc, CanvasNode } from './kernelClient'

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
}

interface CanvasStore {
  tabs: Map<string, CanvasTabState>
  setLoading: (relpath: string) => void
  setDoc: (relpath: string, doc: CanvasDoc) => void
  setError: (relpath: string, error: string) => void
  clear: (relpath: string) => void
  get: (relpath: string) => CanvasTabState | undefined
  setCamera: (relpath: string, camera: Camera) => void
  setSelection: (relpath: string, ids: string[]) => void
  /** Apply a partial mutator to the doc. Caller is responsible for
   *  sending the matching patch to the kernel. */
  updateDoc: (relpath: string, fn: (doc: CanvasDoc) => CanvasDoc) => void
}

const emptyState: CanvasTabState = {
  doc: null,
  loading: false,
  error: null,
  camera: DEFAULT_CAMERA,
  cameraInitialized: false,
  selection: new Set(),
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
    set((s) => ({ tabs: produce(s.tabs, relpath, { selection: new Set(ids) }) })),
  updateDoc: (relpath, fn) =>
    set((s) => {
      const prev = s.tabs.get(relpath)?.doc
      if (!prev) return s
      return { tabs: produce(s.tabs, relpath, { doc: fn(prev) }) }
    }),
}))

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
