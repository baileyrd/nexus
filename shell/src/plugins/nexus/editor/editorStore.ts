import { create } from 'zustand'

/**
 * Preview vs source view mode, per tab. `preview` renders the file's
 * content (markdown → sanitised HTML, otherwise a raw <pre>).
 * `source` swaps in an editable textarea over the raw content.
 */
export type EditorTabMode = 'preview' | 'source'

/**
 * One open file buffer in the tab collection. `relpath` is
 * forge-relative, forward-slash separated (matches the payload
 * emitted by `nexus.files` on `files:open`) and doubles as the tab's
 * identity. `content` is the decoded UTF-8 text — or a sentinel
 * string when the bytes couldn't be decoded as UTF-8.
 *
 * `savedContent` tracks the last-known-on-disk value; equality with
 * `content` means the tab is clean. `mode` defaults to `preview` on
 * open and flips per-tab via the mode-toggle button.
 *
 * Loading / error live per-tab so a failed load on tab X doesn't
 * clobber the healthy content of tab Y.
 */
export interface EditorTab {
  relpath: string
  name: string
  content: string
  /** Last-known-on-disk content. `content === savedContent` ⇒ clean. */
  savedContent: string
  /** Per-tab preview/source mode. Defaults to `preview` on open. */
  mode: EditorTabMode
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
   * read. Newly-created tabs start in `loading: true` + `mode: 'preview'`.
   */
  openTab: (relpath: string, name: string) => boolean
  /**
   * Resolve a successful load — clears loading + error for the tab
   * and seeds BOTH `content` and `savedContent` so a freshly-loaded
   * tab arrives clean.
   */
  setTabContent: (relpath: string, content: string) => void
  /** Resolve a failed load — clears loading, stores the error string. */
  setTabError: (relpath: string, error: string) => void
  /**
   * Mutate the editable `content` only — does NOT touch
   * `savedContent`. This is what turns a tab dirty (when the
   * textarea diverges from disk).
   */
  setContent: (relpath: string, content: string) => void
  /**
   * Acknowledge a successful write: `savedContent = content`. Called
   * by the save command after the kernel confirms the write.
   */
  markSaved: (relpath: string) => void
  /** Flip per-tab preview/source mode. */
  setMode: (relpath: string, mode: EditorTabMode) => void
  /**
   * Remove a tab. If it was active, picks a new active tab:
   *   right neighbour → left neighbour → null.
   */
  closeTab: (relpath: string) => void
  setActive: (relpath: string) => void
  /**
   * Create a new in-memory tab not yet backed by disk. `relpath` is
   * expected to be a non-colliding placeholder like `untitled-1`;
   * callers are responsible for picking the next free number.
   */
  openUntitled: (relpath: string, name: string) => void
  /** Wipe all tabs — used on `workspace:closed`. */
  clear: () => void
}

/**
 * Plain predicate for callers that don't need a live hook
 * subscription — notably the command handler / context-key
 * subscription side. Kept outside the store so it can be imported
 * without pulling Zustand reactivity along.
 */
export function isDirty(tab: EditorTab): boolean {
  return tab.content !== tab.savedContent
}

/**
 * Multi-file editor state. A `files:open` event either raises an
 * existing tab (same relpath) or appends a new one; the active tab
 * drives the render in `EditorView`. Each tab carries its own
 * preview/source mode and a last-saved snapshot for dirty tracking.
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
      savedContent: '',
      mode: 'preview',
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
          ? { ...t, content, savedContent: content, loading: false, error: null }
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

  setContent: (relpath, content) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.relpath === relpath ? { ...t, content } : t,
      ),
    })),

  markSaved: (relpath) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.relpath === relpath ? { ...t, savedContent: t.content } : t,
      ),
    })),

  setMode: (relpath, mode) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.relpath === relpath ? { ...t, mode } : t,
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

  openUntitled: (relpath, name) => {
    const tab: EditorTab = {
      relpath,
      name,
      content: '',
      savedContent: '',
      mode: 'preview',
      loading: false,
      error: null,
    }
    set((s) => ({ tabs: [...s.tabs, tab], activeRelpath: relpath }))
  },

  clear: () => set({ tabs: [], activeRelpath: null }),
}))
