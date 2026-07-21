// BL-050 Phase 2 — side-margin comments pane.
//
// Activation lifecycle mirrors the Backlinks / Outline plugins:
//   1. Register the workspace View under `viewType: 'comments'`.
//   2. Advertise a right-panel tab so the rightPanel host can show us.
//   3. On editor active-tab changes, reload the thread list for the
//      newly-active relpath. Tab-switch races are guarded by a
//      monotonic request id.
//   4. Expose a focus command so users can pop the pane to the front
//      via the command palette.
//
// Phase 2 scope:
//   - Read existing threads and surface reply / resolve / edit /
//     delete actions through `com.nexus.comments` IPC.
//   - The thread-creation IPC is bound (and exported via
//     `commentsApi`) but the pane does not render a "new thread"
//     affordance. Creation belongs to the editor margin gutter
//     (Phase 3) which has access to per-block cursor state and can
//     stamp a stable block-id via `com.nexus.editor::stamp_block`
//     before calling `create_thread`.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { CommentsView } from './CommentsView'
import { commentsPaneViewCreator } from './CommentsPaneView'
import { useCommentsStore } from './commentsStore'
import { createCommentsApi, type CommentsApi } from './commentsApi'
import { useEditorStore } from '../editor/editorStore'

const VIEW_ID = 'nexus.comments.view'
const COMMAND_FOCUS = 'nexus.comments.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

/** Files we actually keep comment threads on. Non-markdown tabs (the
 *  files plugin's binary fallthrough, untitled placeholders) don't
 *  have stable block ids to anchor against, so don't try to load. */
function isCommentableRelpath(relpath: string): boolean {
  if (/^untitled-\d+$/i.test(relpath)) return false
  const lower = relpath.toLowerCase()
  return lower.endsWith('.md') || lower.endsWith('.markdown')
}

/** C60 (#413) — decide whether a `com.nexus.comments.*` bus event
 *  (published by every mutating IPC handler, own-window or from a
 *  collab peer / popout / other IPC writer) should trigger a reload of
 *  the currently-displayed thread list. Only true when the event's
 *  `file_path` matches the active tab — an event for a file open in
 *  another window shouldn't switch what this pane displays. Exported
 *  as a pure function so the matching logic is unit-testable without
 *  a kernel bridge. */
export function commentsEventTargetsActiveFile(
  payload: unknown,
  activeRelpath: string | null,
): boolean {
  if (!activeRelpath) return false
  if (!payload || typeof payload !== 'object') return false
  const filePath = (payload as { file_path?: unknown }).file_path
  return typeof filePath === 'string' && filePath === activeRelpath
}

export const commentsPlugin: Plugin = {
  manifest: {
    id: 'nexus.comments',
    name: 'Comments',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Comments', category: 'View' },
      ],
    },
  },

  activate(api: PluginAPI) {
    let cachedApi: CommentsApi | null = null
    const getApi = (): CommentsApi => {
      if (cachedApi) return cachedApi
      cachedApi = createCommentsApi(api.kernel)
      return cachedApi
    }

    api.viewRegistry.register(
      'comments',
      commentsPaneViewCreator(() =>
        createElement(CommentsView, { api: getApi(), author: null }),
      ),
    )

    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Comments',
      priority: 30,
      iconName: 'comment',
    })

    // ── Loader + requestId guard ──────────────────────────────────────
    //
    // Like the Backlinks plugin: tag every list call with a monotonic
    // id and drop late responses whose id is stale. A fast switch
    // between editor tabs would otherwise let the response for file A
    // arrive after the user has already switched to file B.
    let currentRequestId = 0

    const load = async (relpath: string | null) => {
      const store = useCommentsStore.getState()
      if (!relpath || !isCommentableRelpath(relpath)) {
        currentRequestId++
        store.setCurrent(relpath)
        store.setThreads([])
        store.setLoading(false)
        store.setError(null)
        return
      }

      const requestId = ++currentRequestId
      store.setCurrent(relpath)
      store.setThreads([])
      store.setError(null)
      store.setLoading(true)

      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (requestId !== currentRequestId) return
      if (!available) {
        store.setLoading(false)
        store.setError('Kernel not ready.')
        return
      }

      try {
        const threads = await getApi().list(relpath)
        if (requestId !== currentRequestId) return
        useCommentsStore.getState().setThreads(threads)
        useCommentsStore.getState().setLoading(false)
      } catch (err) {
        if (requestId !== currentRequestId) return
        const message = err instanceof Error ? err.message : String(err)
        useCommentsStore.getState().setThreads([])
        useCommentsStore.getState().setError(message)
        useCommentsStore.getState().setLoading(false)
      }
    }

    // Reload on active-tab changes only — comment edits go through
    // IPC and the View applies its own optimistic updates, so we
    // don't need a per-keystroke refresh path. (And in fact comments
    // don't change just because the markdown source did — they're
    // sidecar state.)
    useEditorStore.subscribe((state, prev) => {
      if (state.activeRelpath !== prev.activeRelpath) {
        void load(state.activeRelpath)
      }
    })

    // Seed with whatever is active at activation time. Deferred so
    // `kernel.available()` runs after the host finishes wiring up.
    queueMicrotask(() => {
      const initial = useEditorStore.getState().activeRelpath
      if (initial) void load(initial)
    })

    // C60 (#413) — nexus-comments now publishes `com.nexus.comments.*`
    // on every mutation. Subscribe so collab peers, popout windows, and
    // any other IPC-only writer refresh this pane live instead of only
    // on the next same-window `nexus.comments:reload` emit or full
    // reload. PluginRegistry sweeps the `api.kernel.on` disposer on
    // unload, same as activityTimeline — no manual teardown needed.
    void (async () => {
      try {
        if (!(await api.kernel.available())) return
        await api.kernel.on<{ file_path?: string }>(
          'com.nexus.comments.',
          (_topic, payload) => {
            const active = useEditorStore.getState().activeRelpath
            if (commentsEventTargetsActiveFile(payload, active)) {
              void load(active)
            }
          },
        )
      } catch {
        // Best-effort: same-window IPC + the local reload event still
        // cover the common case if the bus subscribe fails.
      }
    })()

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      currentRequestId++
      useCommentsStore.getState().clear()
    })

    // BL-050 Phase 3 — the editor margin gutter emits this after a
    // successful `create_thread` so the pane reflects the new thread
    // without forcing the user to switch tabs and back. Payload
    // optional: when `relpath` is omitted, fall back to the editor's
    // current active tab so a same-file create still refreshes.
    api.events.on<{ relpath?: string }>('nexus.comments:reload', (payload) => {
      const relpath =
        (payload && typeof payload.relpath === 'string' ? payload.relpath : null) ??
        useEditorStore.getState().activeRelpath
      if (!relpath) return
      void load(relpath)
    })

    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('comments', 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
