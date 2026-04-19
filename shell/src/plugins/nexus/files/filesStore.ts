import { create } from 'zustand'

/**
 * One entry in a forge directory listing. Mirrors the Rust
 * `nexus_storage::TreeEntry` / `nexus_app::forge::ForgeDirEntry` shape
 * (both serialize `#[serde(rename_all = "camelCase")]`).
 *
 * Paths are forge-relative, forward-slash separated. The empty string
 * is the forge root sentinel.
 */
export interface FilesDirEntry {
  name: string
  relpath: string
  isDirectory: boolean
}

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
  setChildren: (relpath: string, entries: FilesDirEntry[]) => void
  toggleExpanded: (relpath: string) => void
  setSelected: (relpath: string | null) => void
  reset: () => void
}

export const useFilesStore = create<FilesState>((set) => ({
  children: {},
  expanded: new Set(),
  selected: null,
  setChildren: (relpath, entries) =>
    set((s) => ({ children: { ...s.children, [relpath]: entries } })),
  toggleExpanded: (relpath) =>
    set((s) => {
      const next = new Set(s.expanded)
      if (next.has(relpath)) next.delete(relpath)
      else next.add(relpath)
      return { expanded: next }
    }),
  setSelected: (relpath) => set({ selected: relpath }),
  reset: () => set({ children: {}, expanded: new Set(), selected: null }),
}))
