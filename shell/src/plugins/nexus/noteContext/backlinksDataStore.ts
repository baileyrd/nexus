// Shared backlinks data for the Note Context panel.
//
// Owned by `nexus.noteContext`. Populated by an always-on subscriber
// in `noteContext.activate()` so the count + loading flag are fresh
// regardless of whether the Backlinks section is currently expanded.
// This deviates from the panel's "hard lazy" lazy-load contract for
// just this one section, deliberately: the backlinks count is also
// surfaced as a passive indicator in `RightPanelFooter` and the
// status-bar `FileStats` slice, which need always-current data.
// A single kernel call per active-file change is cheap; the cost
// of running it always is negligible vs the UX cost of an indicator
// that stops updating.
//
// Consumers:
//   - `BacklinksSection.tsx`           — reads links / loading / error
//   - `RightPanelFooter.tsx`           — reads links.length / loading
//   - `statusBar/FileStats.tsx`        — same
//
// BL-049 phase 4 — when `blockFilter` is set, the loader switches
// from the `backlinks` IPC to `backlinks_to_block` with the same
// path and an extra `block_id` arg. Clearing the filter (back to
// `null`) reverts to the unfiltered call.

import { create } from 'zustand'

export interface Backlink {
  /** Forge-relative path of the file that links TO the current file. */
  sourceRelpath: string
  /** Basename of `sourceRelpath`, computed client-side. */
  sourceName: string
  /** Display text of the inbound link. */
  linkText: string
  /** "wikilink" | "markdown" | "embed" — passed through from the kernel. */
  linkType: string
  /** Anchor fragment carried by the source link (BL-049 phase 3).
   *  `^<block-id>` for block-anchored, heading slug for heading-anchored,
   *  `null` for plain wikilinks. */
  fragment: string | null
}

interface BacklinksDataState {
  /** Forge-relative path of the file whose backlinks we're tracking. */
  currentRelpath: string | null
  /** Current backlinks list (empty while loading or with no active file). */
  links: Backlink[]
  /** True while a kernel call is in flight for the current file. */
  loading: boolean
  /** Human-readable error string when the last load failed, else null. */
  error: string | null
  /** BL-049 phase 4 — when set, narrows the kernel call to
   *  `backlinks_to_block` with this block id (no leading `^`).
   *  Cleared by clicking the active-filter chip's `×`, by switching
   *  to a different file, or by closing the workspace. `null` means
   *  "show every backlink to the current file." */
  blockFilter: string | null

  setCurrent(relpath: string | null): void
  setLinks(ls: Backlink[]): void
  setLoading(b: boolean): void
  setError(e: string | null): void
  setBlockFilter(blockId: string | null): void
  /** Reset everything — used on workspace close. */
  clear(): void
}

export const useBacklinksDataStore = create<BacklinksDataState>((set) => ({
  currentRelpath: null,
  links: [],
  loading: false,
  error: null,
  blockFilter: null,
  setCurrent: (currentRelpath) => set({ currentRelpath }),
  setLinks: (links) => set({ links }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  setBlockFilter: (blockFilter) => set({ blockFilter }),
  clear: () =>
    set({
      currentRelpath: null,
      links: [],
      loading: false,
      error: null,
      blockFilter: null,
    }),
}))
