import { create } from 'zustand'
import type { CanvasDoc } from './kernelClient'

/** Per-relpath load state. Phase 1 stores the full doc + a loading
 *  flag; camera / selection / pending patches come in later phases. */
export interface CanvasTabState {
  doc: CanvasDoc | null
  loading: boolean
  error: string | null
}

interface CanvasStore {
  tabs: Map<string, CanvasTabState>
  setLoading: (relpath: string) => void
  setDoc: (relpath: string, doc: CanvasDoc) => void
  setError: (relpath: string, error: string) => void
  clear: (relpath: string) => void
  get: (relpath: string) => CanvasTabState | undefined
}

const emptyState: CanvasTabState = { doc: null, loading: false, error: null }

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
}))
