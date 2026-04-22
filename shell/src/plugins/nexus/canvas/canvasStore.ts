import { create } from 'zustand'
import type { CanvasDoc } from './kernelClient'

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
}

interface CanvasStore {
  tabs: Map<string, CanvasTabState>
  setLoading: (relpath: string) => void
  setDoc: (relpath: string, doc: CanvasDoc) => void
  setError: (relpath: string, error: string) => void
  clear: (relpath: string) => void
  get: (relpath: string) => CanvasTabState | undefined
  setCamera: (relpath: string, camera: Camera) => void
}

const emptyState: CanvasTabState = {
  doc: null,
  loading: false,
  error: null,
  camera: DEFAULT_CAMERA,
  cameraInitialized: false,
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
}))
