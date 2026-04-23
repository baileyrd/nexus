import { create } from 'zustand'
import type { TransactionId } from './types.ts'

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
   * Highest revision we've observed from the kernel for each open
   * session, keyed by relpath. Populated by the
   * `com.nexus.editor.changed.<relpath>` subscription set up in
   * `sessionManager.acquire` (Phase 4) — and, later, by the Phase 5
   * transaction-bridge snapshot reconciliation.
   *
   * Using a native `Map` rather than a plain object so set/delete are
   * O(1) without Zustand needing to diff an object's full keyspace.
   * Callers must use the setter so Zustand's subscribers see the
   * change (Maps are compared by reference).
   */
  sessionRevision: Map<string, number>
  /**
   * Phase 6: last kernel revision we observed at successful-save time,
   * keyed by relpath. `isDirty` compares this against
   * `sessionRevision` to decide whether a tab diverges from disk. The
   * bridge/subscription advances `sessionRevision` on any local or
   * remote edit; `markSaved` snapshots the current `sessionRevision`
   * into `savedRevision` so the tab goes clean atomically with the
   * write.
   *
   * Tabs without a kernel session (untitled placeholders, non-markdown
   * files for which we never acquire) simply have no entry in either
   * map — `isDirty` falls back to the legacy `content !== savedContent`
   * check for those.
   */
  savedRevision: Map<string, number>
  /**
   * Transaction ids the shell has dispatched locally and is still
   * waiting to see echoed back through the `changed` event. Populated
   * by the Phase 5 transaction bridge *before* `apply_transaction` is
   * invoked; drained by the Phase 4 subscription handler when the
   * echo arrives. Stays empty for Phase 4 in isolation — the set is
   * added here so the subscription code has a stable place to check.
   */
  pendingLocalRevisions: Set<TransactionId>

  /** Replace the known revision for `relpath`. */
  setSessionRevision: (relpath: string, revision: number) => void
  /** Drop any tracked revision for `relpath` (called on release). */
  clearSessionRevision: (relpath: string) => void
  /**
   * Snapshot the current `sessionRevision` for `relpath` into
   * `savedRevision`. Called after a successful save and after an
   * untitled → named transition establishes the session. Also seeds
   * `savedContent := content` for the legacy dirty fallback.
   */
  markSavedRevision: (relpath: string) => void
  /** Drop the saved revision for `relpath` (on tab close / release). */
  clearSavedRevision: (relpath: string) => void
  /**
   * Atomically rename a tab's relpath — used by the untitled → named
   * transition in `COMMAND_SAVE`. Moves any sessionRevision /
   * savedRevision entries keyed by the old path to the new path and
   * re-points `activeRelpath` if needed. Idempotent when
   * `oldRelpath === newRelpath`.
   */
  renameTab: (oldRelpath: string, newRelpath: string, newName?: string) => void
  /**
   * Record a transaction id as in-flight so the subscription handler
   * knows to drop its corresponding echo event. Idempotent — adding
   * the same id twice is harmless.
   */
  addPendingLocalRevision: (transactionId: TransactionId) => void
  /**
   * Remove a transaction id from the pending set. Returns `true` if
   * the id WAS pending (i.e. this is an echo the caller should drop),
   * `false` otherwise.
   */
  consumePendingLocalRevision: (transactionId: TransactionId) => boolean

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
   *
   * **Use only for tabs that are NOT bridge-eligible** (no kernel
   * session — i.e. untitled drafts and non-markdown buffers). For
   * tabs with a kernel session, mutations flow through the
   * transaction bridge (`apply_transaction` IPC) and the store
   * receives them via `setSessionRevision` / sync-back. Calling
   * `setContent` on a session-bound tab bypasses the bridge and
   * desyncs the kernel-side document.
   *
   * Legitimate call sites: `EditorView.tsx` no-session local-mode
   * `<textarea>` fallback (one), `editorStore.test.ts` (two).
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
  /** Set the focused tab. `null` clears active state — used when a
   *  non-markdown workspace leaf becomes active so the status bar
   *  stops showing stats for a file that isn't in focus. */
  setActive: (relpath: string | null) => void
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
 *
 * Phase 6: dirty is now primarily determined by server revision —
 * `sessionRevision[relpath]` vs `savedRevision[relpath]`. The
 * transaction bridge advances `sessionRevision` on every local edit,
 * and `markSaved` snapshots it into `savedRevision` after a
 * successful write. We only fall back to the legacy content-diff
 * check for tabs that have no kernel session (untitled placeholders,
 * non-markdown buffers for which we never acquire).
 */
export function isDirty(tab: EditorTab): boolean {
  const state = useEditorStore.getState()
  const current = state.sessionRevision.get(tab.relpath)
  if (current !== undefined) {
    // SessionManager seeds both maps on acquire so a freshly-opened
    // tab reads as clean; after that, any divergence is a real edit.
    const saved = state.savedRevision.get(tab.relpath)
    if (saved === undefined) {
      // Invariant violation: sessionRevision exists but savedRevision
      // doesn't — acquire failed to seed savedRevision, or some path
      // wrote sessionRevision before acquire. Defensive fallback:
      // assume clean (matches prior `?? current` behaviour) so we
      // don't false-flag every newly-opened tab as dirty, but log so
      // the broken invariant surfaces in dev builds.
      console.warn(
        `[editorStore] isDirty: sessionRevision present but savedRevision missing for '${tab.relpath}' — invariant violation; treating as clean. This indicates a missed acquire seed in SessionManager.`,
      )
      return false
    }
    return current !== saved
  }
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
  sessionRevision: new Map<string, number>(),
  savedRevision: new Map<string, number>(),
  pendingLocalRevisions: new Set<TransactionId>(),

  setSessionRevision: (relpath, revision) =>
    set((s) => {
      const next = new Map(s.sessionRevision)
      next.set(relpath, revision)
      return { sessionRevision: next }
    }),

  clearSessionRevision: (relpath) =>
    set((s) => {
      if (!s.sessionRevision.has(relpath)) return s
      const next = new Map(s.sessionRevision)
      next.delete(relpath)
      return { sessionRevision: next }
    }),

  markSavedRevision: (relpath) =>
    set((s) => {
      const current = s.sessionRevision.get(relpath)
      if (current === undefined) return s
      const next = new Map(s.savedRevision)
      next.set(relpath, current)
      return { savedRevision: next }
    }),

  clearSavedRevision: (relpath) =>
    set((s) => {
      if (!s.savedRevision.has(relpath)) return s
      const next = new Map(s.savedRevision)
      next.delete(relpath)
      return { savedRevision: next }
    }),

  renameTab: (oldRelpath, newRelpath, newName) =>
    set((s) => {
      if (oldRelpath === newRelpath) {
        if (!newName) return s
        return {
          tabs: s.tabs.map((t) =>
            t.relpath === oldRelpath ? { ...t, name: newName } : t,
          ),
        }
      }
      const tabs = s.tabs.map((t) =>
        t.relpath === oldRelpath
          ? { ...t, relpath: newRelpath, name: newName ?? t.name }
          : t,
      )
      const activeRelpath =
        s.activeRelpath === oldRelpath ? newRelpath : s.activeRelpath

      const remap = <V,>(src: Map<string, V>): Map<string, V> => {
        if (!src.has(oldRelpath)) return src
        const next = new Map(src)
        const val = next.get(oldRelpath) as V
        next.delete(oldRelpath)
        next.set(newRelpath, val)
        return next
      }
      return {
        tabs,
        activeRelpath,
        sessionRevision: remap(s.sessionRevision),
        savedRevision: remap(s.savedRevision),
      }
    }),

  addPendingLocalRevision: (transactionId) =>
    set((s) => {
      if (s.pendingLocalRevisions.has(transactionId)) return s
      const next = new Set(s.pendingLocalRevisions)
      next.add(transactionId)
      return { pendingLocalRevisions: next }
    }),

  consumePendingLocalRevision: (transactionId) => {
    const current = get().pendingLocalRevisions
    if (!current.has(transactionId)) return false
    const next = new Set(current)
    next.delete(transactionId)
    set({ pendingLocalRevisions: next })
    return true
  },

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
    set((s) => {
      // Legacy content snapshot (still used for the untitled / no-session
      // fallback path in `isDirty`).
      const tabs = s.tabs.map((t) =>
        t.relpath === relpath ? { ...t, savedContent: t.content } : t,
      )
      // Phase 6: promote the current kernel revision to "saved". If
      // the tab has no session (untitled save-then-open race, or a
      // non-markdown write) we just skip the revision update — the
      // content snapshot above is enough to keep the tab clean.
      const current = s.sessionRevision.get(relpath)
      if (current === undefined) return { tabs }
      const nextSaved = new Map(s.savedRevision)
      nextSaved.set(relpath, current)
      return { tabs, savedRevision: nextSaved }
    }),

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
      // Phase 6: drop the saved-revision entry. The session-revision
      // entry is managed by `sessionManager.release` which already
      // fires from the index.ts subscribe handler.
      let savedRevision = s.savedRevision
      if (savedRevision.has(relpath)) {
        const next = new Map(savedRevision)
        next.delete(relpath)
        savedRevision = next
      }
      return { tabs: nextTabs, activeRelpath: nextActive, savedRevision }
    }),

  setActive: (relpath) => {
    if (relpath === null) {
      set({ activeRelpath: null })
      return
    }
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

  clear: () =>
    set({
      tabs: [],
      activeRelpath: null,
      sessionRevision: new Map<string, number>(),
      savedRevision: new Map<string, number>(),
      pendingLocalRevisions: new Set<TransactionId>(),
    }),
}))
