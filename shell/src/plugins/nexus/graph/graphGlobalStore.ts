import { create } from 'zustand'

export interface GlobalGraphNode {
  path: string
  isPhantom: boolean
}

export interface GlobalGraphEdge {
  source: string
  target: string
  isResolved: boolean
}

export interface GlobalGraphSettings {
  // Filters
  pathFilter: string
  includeUnresolved: boolean
  includeOrphans: boolean
  // Display
  showLabels: boolean
  // Forces
  linkDistance: number
  linkStrength: number
  repulsion: number
  centerGravity: number
  // Simulation
  freeze: boolean
  // Groups: simple folder→colour hash; toggle on/off.
  colourByFolder: boolean
}

export const DEFAULT_SETTINGS: GlobalGraphSettings = {
  pathFilter: '',
  includeUnresolved: true,
  includeOrphans: true,
  showLabels: true,
  linkDistance: 60,
  linkStrength: 0.18,
  repulsion: 220,
  centerGravity: 0.02,
  freeze: false,
  colourByFolder: false,
}

interface GlobalGraphState {
  nodes: GlobalGraphNode[]
  edges: GlobalGraphEdge[]
  loadedAt: number | null
  loading: boolean
  error: string | null
  settings: GlobalGraphSettings

  setSnapshot(nodes: GlobalGraphNode[], edges: GlobalGraphEdge[]): void
  setLoading(b: boolean): void
  setError(e: string | null): void
  patchSettings(p: Partial<GlobalGraphSettings>): void
  resetSettings(): void
  clear(): void
}

export const useGlobalGraphStore = create<GlobalGraphState>((set) => ({
  nodes: [],
  edges: [],
  loadedAt: null,
  loading: false,
  error: null,
  settings: { ...DEFAULT_SETTINGS },

  setSnapshot: (nodes, edges) =>
    set({ nodes, edges, loadedAt: Date.now(), error: null }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  patchSettings: (p) =>
    set((s) => ({ settings: { ...s.settings, ...p } })),
  resetSettings: () => set({ settings: { ...DEFAULT_SETTINGS } }),
  clear: () =>
    set({
      nodes: [],
      edges: [],
      loadedAt: null,
      loading: false,
      error: null,
    }),
}))
