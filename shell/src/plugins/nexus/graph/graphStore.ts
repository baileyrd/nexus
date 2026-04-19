import { create } from 'zustand'

/**
 * Which side(s) of the link relationship a neighbour sits on relative
 * to the current file.
 *
 * - `outgoing`: the current file links to the neighbour
 *   (neighbour appears as a target in outgoing_links).
 * - `incoming`: the neighbour links to the current file
 *   (neighbour appears as a source in backlinks).
 * - `both`: a bidirectional relationship — both sides exist in the
 *   index for the same neighbour relpath.
 *
 * The graph view can style each direction differently later; for now
 * direction is tracked but not drawn with distinct strokes/arrows.
 */
export type EdgeDirection = 'outgoing' | 'incoming' | 'both'

/**
 * A single file that shares a 1-hop link with the currently-open file.
 *
 * `relpath` is forge-relative, forward-slash separated — matches the
 * payload shape used across the rest of the shell (files:open,
 * editor tab identity, etc.). `name` is `basename(relpath)`, computed
 * client-side so the view doesn't need to re-derive it.
 */
export interface GraphNeighbour {
  relpath: string
  name: string
  direction: EdgeDirection
}

interface GraphState {
  /** Forge-relative path of the file at the centre of the neighbourhood. */
  currentRelpath: string | null
  /** Display name of the centre file — basename of `currentRelpath`. */
  currentName: string | null
  /** 1-hop neighbours, merged from outgoing_links + backlinks. */
  neighbours: GraphNeighbour[]
  /** True while a parallel outgoing/backlinks fetch is in flight. */
  loading: boolean
  /** Human-readable error string when the last load failed, else null. */
  error: string | null

  setCurrent(relpath: string | null, name: string | null): void
  setNeighbours(ns: GraphNeighbour[]): void
  setLoading(b: boolean): void
  setError(e: string | null): void
  /** Reset everything — used on workspace close. */
  clear(): void
}

/**
 * Zustand store backing the Graph right-panel tab.
 *
 * The loader in `index.ts` drives all state transitions; the view
 * reads `currentRelpath`, `currentName`, `neighbours`, `loading`,
 * `error` and renders accordingly.
 */
export const useGraphStore = create<GraphState>((set) => ({
  currentRelpath: null,
  currentName: null,
  neighbours: [],
  loading: false,
  error: null,
  setCurrent: (currentRelpath, currentName) =>
    set({ currentRelpath, currentName }),
  setNeighbours: (neighbours) => set({ neighbours }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  clear: () =>
    set({
      currentRelpath: null,
      currentName: null,
      neighbours: [],
      loading: false,
      error: null,
    }),
}))
