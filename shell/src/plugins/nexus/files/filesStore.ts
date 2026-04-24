import { create } from 'zustand'

/**
 * One entry in a forge directory listing. Mirrors the Rust
 * `nexus_storage::TreeEntry` shape (serialized
 * `#[serde(rename_all = "camelCase")]`). The legacy shell's
 * `nexus_app::forge::ForgeDirEntry` twin was retired under Phase 4 WI-37.
 *
 * Paths are forge-relative, forward-slash separated. The empty string
 * is the forge root sentinel.
 */
export interface FilesDirEntry {
  name: string
  relpath: string
  /** Matches the Rust `TreeEntry.is_dir` field serialized with
   *  `#[serde(rename_all = "camelCase")]`, which produces `isDir` on
   *  the wire. */
  isDir: boolean
  /** Last-modified time (unix millis). Absent when the filesystem /
   *  platform doesn't expose it. */
  modifiedMs?: number
  /** Created time (unix millis). Absent on filesystems that don't
   *  track birth time (Linux w/o statx, some network shares). */
  createdMs?: number
}

export type SortMode =
  | 'nameAsc'
  | 'nameDesc'
  | 'modifiedDesc'
  | 'modifiedAsc'
  | 'createdDesc'
  | 'createdAsc'

interface FilesState {
  /**
   * Directory-listing cache keyed by forge-relative path. The empty
   * string `""` is the root sentinel. A directory is absent from this
   * map until it has been successfully listed at least once.
   */
  children: Record<string, FilesDirEntry[]>
  /** Set of directory relpaths currently expanded in the tree UI. */
  expanded: Set<string>
  /** Currently selected file relpath, or null. Purely visual — no file is "open" yet until an editor plugin consumes the `files:open` event. */
  selected: string | null
  /** Tree-wide sort order. Dirs always come before files regardless of mode. */
  sortMode: SortMode
  /** When true, the tree scrolls + expands to the active editor file. */
  autoReveal: boolean
  setChildren: (relpath: string, entries: FilesDirEntry[]) => void
  toggleExpanded: (relpath: string) => void
  setExpanded: (relpath: string, expanded: boolean) => void
  collapseAll: () => void
  setSelected: (relpath: string | null) => void
  setSortMode: (mode: SortMode) => void
  setAutoReveal: (on: boolean) => void
  reset: () => void
}

export const useFilesStore = create<FilesState>((set) => ({
  children: {},
  expanded: new Set(),
  selected: null,
  sortMode: 'nameAsc',
  autoReveal: false,
  setChildren: (relpath, entries) =>
    set((s) => ({ children: { ...s.children, [relpath]: entries } })),
  toggleExpanded: (relpath) =>
    set((s) => {
      const next = new Set(s.expanded)
      if (next.has(relpath)) next.delete(relpath)
      else next.add(relpath)
      return { expanded: next }
    }),
  setExpanded: (relpath, expanded) =>
    set((s) => {
      const next = new Set(s.expanded)
      if (expanded) next.add(relpath)
      else next.delete(relpath)
      return { expanded: next }
    }),
  collapseAll: () => set({ expanded: new Set() }),
  setSelected: (relpath) => set({ selected: relpath }),
  setSortMode: (mode) => set({ sortMode: mode }),
  setAutoReveal: (on) => set({ autoReveal: on }),
  reset: () =>
    set((s) => ({
      children: {},
      expanded: new Set(),
      selected: null,
      // Preserve user-chosen sort + auto-reveal across workspace swaps.
      sortMode: s.sortMode,
      autoReveal: s.autoReveal,
    })),
}))
