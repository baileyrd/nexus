import { create } from 'zustand'

/**
 * A single file that links TO the currently-open file. Derived from
 * the kernel's `BacklinkResult` ({ source_path, link_text, link_type })
 * by basename-ing `source_path` client-side.
 *
 * The kernel does not return line numbers or excerpts — backlinks are
 * graph-edge metadata, not search hits — so those fields are absent
 * here. We surface `linkText` (the display text of the wikilink /
 * markdown link that pointed at us) in place of a content excerpt.
 */
export interface Backlink {
  /** Forge-relative path of the file that links TO the current file. */
  sourceRelpath: string
  /** Basename of `sourceRelpath`, computed client-side. */
  sourceName: string
  /** Display text of the inbound link (e.g. "My Note" or "foo/bar.md"). */
  linkText: string
  /** "wikilink" | "markdown" | "embed" — passed through from the kernel. */
  linkType: string
  /** BL-049 phase 3 — anchor fragment carried by the source link.
   *  `^<block-id>` for block-anchored links, the heading slug for
   *  heading-anchored links, `null` for plain wikilinks. The view
   *  surfaces a small pill so the user can tell *which* block /
   *  section the source pointed at. */
  fragment: string | null
}

interface BacklinksState {
  /** Forge-relative path of the file whose backlinks we're showing. */
  currentRelpath: string | null
  /** Current list of backlinks. Empty when loading / no active file / none found. */
  links: Backlink[]
  /** True while a kernel call is in flight for the current file. */
  loading: boolean
  /** Human-readable error string when the last load failed, else null. */
  error: string | null

  setCurrent(relpath: string | null): void
  setLinks(ls: Backlink[]): void
  setLoading(b: boolean): void
  setError(e: string | null): void
  /** Reset everything — used on workspace close. */
  clear(): void
}

export const useBacklinksStore = create<BacklinksState>((set) => ({
  currentRelpath: null,
  links: [],
  loading: false,
  error: null,
  setCurrent: (currentRelpath) => set({ currentRelpath }),
  setLinks: (links) => set({ links }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  clear: () =>
    set({ currentRelpath: null, links: [], loading: false, error: null }),
}))
