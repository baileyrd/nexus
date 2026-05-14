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
  /** BL-053 mockup row N — approximate word count for the section
   *  this heading owns (from this heading's line through to the
   *  next heading's line, exclusive of the heading text itself).
   *  Populated by [`parseHeadings`] / [`treeToHeadings`]; omitted
   *  when the parser couldn't compute one (defensive default).
   *  The outline row's faint badge reads this. */
  wordCount?: number
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
  /** Filter string applied to the outline; empty = show all. */
  filter: string
  /** Set of collapsed heading ids. Default-expanded tree — only
   *  explicitly-collapsed branches hide their children. */
  collapsed: Set<string>
  /** When true, scrolling in the editor tracks the outline selection
   *  and auto-expands parent branches. */
  autoScroll: boolean
  setHeadings: (hs: OutlineHeading[]) => void
  setActiveIndex: (i: number | null) => void
  setFilter: (q: string) => void
  toggleCollapsed: (id: string) => void
  collapseAll: () => void
  expandAll: () => void
  setAutoScroll: (on: boolean) => void
  clear: () => void
}

export const useOutlineStore = create<OutlineState>((set) => ({
  headings: [],
  activeIndex: null,
  filter: '',
  collapsed: new Set(),
  autoScroll: true,
  setHeadings: (headings) =>
    // Drop any collapsed ids that no longer resolve to a heading so
    // stale state from a prior document doesn't survive a recompute.
    set((s) => {
      const valid = new Set(headings.map((h) => h.id))
      const next = new Set<string>()
      for (const id of s.collapsed) if (valid.has(id)) next.add(id)
      return { headings, collapsed: next }
    }),
  setActiveIndex: (activeIndex) => set({ activeIndex }),
  setFilter: (filter) => set({ filter }),
  toggleCollapsed: (id) =>
    set((s) => {
      const next = new Set(s.collapsed)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return { collapsed: next }
    }),
  collapseAll: () =>
    set((s) => ({
      // Collapse every heading that has at least one child (is a
      // parent of the next heading in the flat list).
      collapsed: new Set(
        s.headings
          .filter((h, i) => {
            const next = s.headings[i + 1]
            return next !== undefined && next.level > h.level
          })
          .map((h) => h.id),
      ),
    })),
  expandAll: () => set({ collapsed: new Set() }),
  setAutoScroll: (autoScroll) => set({ autoScroll }),
  clear: () =>
    set((s) => ({
      headings: [],
      activeIndex: null,
      filter: '',
      collapsed: new Set(),
      // Preserve the user's auto-scroll preference across documents.
      autoScroll: s.autoScroll,
    })),
}))
