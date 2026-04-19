import { create } from 'zustand'

export interface FilesDirEntry {
  name: string
  path: string
  isDirectory: boolean
}

interface FilesState {
  /** Map from absolute directory path → its immediate children. Empty string is the root sentinel before a workspace is opened. */
  children: Record<string, FilesDirEntry[]>
  /** Set of absolute directory paths currently expanded. */
  expanded: Set<string>
  /** Currently selected file path, or null. Purely visual — no file is "open" yet until an editor plugin consumes the files:open event. */
  selected: string | null
  setChildren: (path: string, entries: FilesDirEntry[]) => void
  toggleExpanded: (path: string) => void
  setSelected: (path: string | null) => void
  reset: () => void
}

export const useFilesStore = create<FilesState>((set) => ({
  children: {},
  expanded: new Set(),
  selected: null,
  setChildren: (path, entries) =>
    set((s) => ({ children: { ...s.children, [path]: entries } })),
  toggleExpanded: (path) =>
    set((s) => {
      const next = new Set(s.expanded)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return { expanded: next }
    }),
  setSelected: (path) => set({ selected: path }),
  reset: () => set({ children: {}, expanded: new Set(), selected: null }),
}))
