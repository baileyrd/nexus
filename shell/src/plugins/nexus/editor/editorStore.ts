import { create } from 'zustand'

/**
 * One open file buffer in the tab collection. `relpath` is
 * forge-relative, forward-slash separated (matches the payload
 * emitted by `nexus.files` on `files:open`) and doubles as the tab's
 * identity. `content` is the decoded UTF-8 text — or a sentinel
 * string when the bytes couldn't be decoded as UTF-8.
 *
 * Loading / error live per-tab so a failed load on tab X doesn't
 * clobber the healthy content of tab Y.
 */
export interface EditorTab {
  relpath: string
  name: string
  content: string
  loading: boolean
  error: string | null
}

interface EditorState {
  tabs: EditorTab[]
  /** null when no tabs are open. */
  activeRelpath: string | null

  /**
   * Add (or raise) a tab for `relpath`. When the tab already exists
   * it simply becomes active — no refetch. Returns `true` if a new
   * tab was created so the caller knows whether to kick off the
   * read. Newly-created tabs start in `loading: true`.
   */
  openTab: (relpath: string, name: string) => boolean
  /** Resolve a successful load — clears loading + error for the tab. */
  setTabContent: (relpath: string, content: string) => void
  /** Resolve a failed load — clears loading, stores the error string. */
  setTabError: (relpath: string, error: string) => void
  /**
   * Remove a tab. If it was active, picks a new active tab:
   *   right neighbour → left neighbour → null.
   */
  closeTab: (relpath: string) => void
  setActive: (relpath: string) => void
  /** Wipe all tabs — used on `workspace:closed`. */
  clear: () => void
}

/**
 * Multi-file read-only editor state. A `files:open` event either
 * raises an existing tab (same relpath) or appends a new one; the
 * active tab drives the render in `EditorView`.
 */
export const useEditorStore = create<EditorState>((set, get) => ({
  tabs: [],
  activeRelpath: null,

  openTab: (relpath, name) => {
    const existing = get().tabs.find((t) => t.relpath === relpath)
    if (existing) {
      if (get().activeRelpath !== relpath) set({ activeRelpath: relpath })
      return false
    }
    const tab: EditorTab = {
      relpath,
      name,
      content: '',
      loading: true,
      error: null,
    }
    set((s) => ({ tabs: [...s.tabs, tab], activeRelpath: relpath }))
    return true
  },

  setTabContent: (relpath, content) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.relpath === relpath
          ? { ...t, content, loading: false, error: null }
          : t,
      ),
    })),

  setTabError: (relpath, error) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.relpath === relpath
          ? { ...t, error, loading: false }
          : t,
      ),
    })),

  closeTab: (relpath) =>
    set((s) => {
      const idx = s.tabs.findIndex((t) => t.relpath === relpath)
      if (idx === -1) return s
      const nextTabs = s.tabs.filter((_, i) => i !== idx)
      let nextActive = s.activeRelpath
      if (s.activeRelpath === relpath) {
        // Right-neighbour preferred (same index in the reduced array),
        // then left-neighbour, else nothing open.
        const right = nextTabs[idx]
        const left = nextTabs[idx - 1]
        nextActive = right?.relpath ?? left?.relpath ?? null
      }
      return { tabs: nextTabs, activeRelpath: nextActive }
    }),

  setActive: (relpath) => {
    if (get().tabs.some((t) => t.relpath === relpath)) {
      set({ activeRelpath: relpath })
    }
  },

  clear: () => set({ tabs: [], activeRelpath: null }),
}))
