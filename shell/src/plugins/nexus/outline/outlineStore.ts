import { create } from 'zustand'

/**
 * A single heading extracted from the active editor tab. `id` is a
 * slug-per-doc identity (level-slug-indexInDoc) so duplicate heading
 * texts don't collide. `line` is 1-based and matches the source
 * markdown line — the editor will use it later for scroll-to.
 */
export interface OutlineHeading {
  id: string
  text: string
  level: number
  line: number
  /** Sequential 0-based index among all headings in the document.
   *  Used by the editor to find the matching DOM element (the Nth
   *  h1..h6 in the rendered markdown body) without needing ids to
   *  match between our slugs and marked's auto-generated ones. */
  index: number
}

interface OutlineState {
  headings: OutlineHeading[]
  /**
   * Index of the heading currently active in the editor (the topmost
   * heading at or above the visible region). `null` when there are no
   * headings or the editor hasn't reported a position yet. The editor
   * publishes via `editor:activeHeadingChanged`; outline/index.ts
   * forwards into here.
   */
  activeIndex: number | null
  setHeadings: (hs: OutlineHeading[]) => void
  setActiveIndex: (i: number | null) => void
  clear: () => void
}

export const useOutlineStore = create<OutlineState>((set) => ({
  headings: [],
  activeIndex: null,
  setHeadings: (headings) => set({ headings }),
  setActiveIndex: (activeIndex) => set({ activeIndex }),
  clear: () => set({ headings: [], activeIndex: null }),
}))
