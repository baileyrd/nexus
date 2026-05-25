import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { OutlineView } from './OutlineView'
import { outlinePaneViewCreator } from './OutlinePaneView'
import { useOutlineStore } from './outlineStore'
import { parseHeadings, treeToHeadings } from './parse'
import { useEditorStore } from '../editor/editorStore'
import { getEditorRuntime } from '../editor/runtime'
import type { BlockTree, EditorChangedPayload } from '../editor/types'

const VIEW_ID = 'nexus.outline.view'
const COMMAND_FOCUS = 'nexus.outline.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'
const EVENT_ACTIVE_HEADING_CHANGED = 'editor:activeHeadingChanged'
/** Fired by `OutlineView` on mount so the plugin forces a recompute —
 *  covers the case where the view was hidden during the last tab
 *  transition (sidedock tab switch, dock collapse) and therefore
 *  missed the store-driven recompute. */
const EVENT_OUTLINE_REQUEST_REFRESH = 'nexus.outline:requestRefresh'

export const OUTLINE_EVENT_REQUEST_REFRESH = EVENT_OUTLINE_REQUEST_REFRESH

interface ActiveHeadingPayload {
  index: number | null
}

/**
 * Rough test for "relpath could have an editor session".
 * Matches the same suffix set the editor plugin considers markdown
 * (`.md` / `.markdown`) — only those paths get a kernel session we
 * can subscribe to for change events. Non-markdown (code / binary)
 * tabs still fall through to the `openUntitled` placeholder path
 * below, which has no session and therefore no outline refresh.
 */
function isMarkdownRelpath(name: string): boolean {
  const lower = name.toLowerCase()
  return lower.endsWith('.md') || lower.endsWith('.markdown')
}

function isUntitledRelpath(relpath: string): boolean {
  return /^untitled-\d+$/i.test(relpath)
}

export const outlinePlugin: Plugin = {
  manifest: {
    id: 'nexus.outline',
    name: 'Outline',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    // Imports `../editor/editorStore`, `../editor/runtime`, and
    // `../editor/types` directly — editor plugin must be loaded first.
    dependsOn: ['nexus.rightPanel', 'nexus.editor'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Outline', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    // Phase 7: legacy SlotRegistry slot:'rightPanelContent' entry removed.
    api.viewRegistry.register(
      'outline',
      outlinePaneViewCreator(() => createElement(OutlineView)),
    )

    // And advertise its tab label to the rightPanel host. The host
    // auto-activates the first-registered tab, so outline — being
    // the only contributor right now — becomes the default.
    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Outline',
      priority: 10,
      iconName: 'list',
    })

    // ── Phase 7 wiring ────────────────────────────────────────────────
    //
    // The outline now derives from the kernel's canonical BlockTree,
    // not from `tab.content`. Lifecycle:
    //   1. On activation, subscribe to the editor runtime's
    //      `sessionManager.onChanged` — fires when the Rust plugin
    //      publishes `com.nexus.editor.changed.<relpath>` (after echo
    //      suppression). Any change for the active relpath debounces
    //      into a `recompute()`.
    //   2. On active-tab change (a different subscription to
    //      `useEditorStore`), recompute immediately for the new tab.
    //   3. `recompute()` calls `kernelClient.getTree(relpath)`, walks
    //      the tree's root_blocks, and emits an `OutlineHeading[]`.
    //
    // Rate-limit recomputes to one per animation frame so a
    // keystroke-per-ms burst (paste / IME) collapses into a single
    // kernel round-trip.

    let rafHandle: number | null = null
    let generation = 0
    const scheduleRecompute = () => {
      if (rafHandle !== null) return
      rafHandle = requestAnimationFrame(() => {
        rafHandle = null
        void recompute()
      })
    }

    /**
     * Pull the latest tree for the active tab and publish headings.
     * `generation` guards against a stale getTree resolution arriving
     * after the user has switched tabs — late responses are dropped.
     */
    const recompute = async () => {
      const myGen = ++generation
      const s = useEditorStore.getState()
      const relpath = s.activeRelpath
      const tab = s.tabs.find((t) => t.relpath === relpath)

      // No active tab, or an untitled placeholder / non-markdown tab:
      // there's no kernel session we could read a tree from. Clear.
      if (!tab || !relpath || isUntitledRelpath(relpath)) {
        useOutlineStore.getState().clear()
        return
      }
      if (!isMarkdownRelpath(tab.name) && !isMarkdownRelpath(relpath)) {
        useOutlineStore.getState().clear()
        return
      }

      // Tab failed to load (missing file / no kernel session — e.g. a
      // restored tab from another vault). There's no session to read a
      // tree from, so skip the kernel fetch (which would fail with "no
      // open session" and get logged) and parse whatever content we have.
      if (tab.error) {
        useOutlineStore.getState().setHeadings(parseHeadings(tab.content))
        useOutlineStore.getState().setActiveIndex(null)
        return
      }

      const runtime = getEditorRuntime()
      if (!runtime) {
        // Editor plugin hasn't finished activating yet. Fall back to
        // the legacy content parse so the first paint isn't empty;
        // the next scheduleRecompute (driven by changes or tab
        // switches) will take the kernel path.
        useOutlineStore
          .getState()
          .setHeadings(parseHeadings(tab.content))
        useOutlineStore.getState().setActiveIndex(null)
        return
      }

      let tree: BlockTree | null = null
      let markdown: string | null = null
      try {
        // Fetch tree (primary source) + markdown (for source-mode line
        // hints) in parallel. Both come from the same session so they
        // represent a coherent snapshot.
        const [snapshot, md] = await Promise.all([
          runtime.kernelClient.getTree(relpath),
          runtime.kernelClient.getMarkdown(relpath),
        ])
        tree = snapshot.tree
        markdown = md
      } catch {
        // Session may have closed between `changed` firing and our
        // fetch, or the kernel is tearing down. Fall through to the
        // content-based parse so the outline stays populated with
        // the last-known state.
        useOutlineStore
          .getState()
          .setHeadings(parseHeadings(tab.content))
        useOutlineStore.getState().setActiveIndex(null)
        return
      }

      // Drop late responses after a fast tab switch.
      if (myGen !== generation) return

      // Recover 1-based source line numbers from the canonical
      // markdown so source-mode scroll-to-heading (CM viewToLine)
      // keeps working. The tree walker already agrees with
      // `parseHeadings` on ordering — both skip fenced code blocks
      // and both honour document order — so a positional zip by
      // index is safe.
      const lineHints: number[] = []
      if (markdown) {
        const parsed = parseHeadings(markdown)
        for (const h of parsed) lineHints.push(h.line)
      }

      const headings = treeToHeadings(tree, lineHints)
      useOutlineStore.getState().setHeadings(headings)
      // Reset activeIndex — the editor's scroll-spy will re-emit a
      // fresh `activeHeadingChanged` once it runs against the new
      // rendered body.
      useOutlineStore.getState().setActiveIndex(null)
    }

    // Subscribe to the editor runtime's changed-event emitter. The
    // runtime is set up synchronously by `editor/index.ts::activate`;
    // our `dependsOn: ['nexus.rightPanel']` doesn't order us after
    // the editor, so we microtask-defer the subscribe to let the
    // editor plugin's activate() finish first.
    let unsubscribeChanged: (() => void) | null = null
    queueMicrotask(() => {
      const runtime = getEditorRuntime()
      if (!runtime) return
      unsubscribeChanged = runtime.sessionManager.onChanged(
        (payload: EditorChangedPayload) => {
          // Ignore events for any relpath other than the active tab —
          // the outline only ever describes one file at a time.
          const active = useEditorStore.getState().activeRelpath
          if (payload.relpath !== active) return
          scheduleRecompute()
        },
      )
    })

    // Active-tab transitions recompute immediately (no rAF debounce —
    // the user just switched files and the UX expects the outline to
    // catch up in one paint). Content-level edits route through the
    // change-event path above.
    const unsubscribeStore = useEditorStore.subscribe((state, prev) => {
      if (state.activeRelpath !== prev.activeRelpath) {
        void recompute()
        return
      }
      // Re-recompute when the active tab finishes loading. On fresh
      // boot `openTab` sets activeRelpath synchronously (firing a
      // recompute) before `sessionManager.acquire` resolves, so the
      // first recompute's getTree returns empty and parseHeadings
      // runs against empty tab.content. Without this hook the outline
      // would stay empty until the user edits or re-opens the file.
      const ap = state.activeRelpath
      if (!ap) return
      const curTab = state.tabs.find((t) => t.relpath === ap)
      const prevTab = prev.tabs.find((t) => t.relpath === ap)
      if (curTab && prevTab && prevTab.loading && !curTab.loading) {
        void recompute()
      }
    })

    // Seed with whatever is active right now.
    void recompute()

    api.events.on(EVENT_OUTLINE_REQUEST_REFRESH, () => {
      void recompute()
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      generation++
      if (rafHandle !== null) {
        cancelAnimationFrame(rafHandle)
        rafHandle = null
      }
      useOutlineStore.getState().clear()
    })

    api.events.on<ActiveHeadingPayload>(EVENT_ACTIVE_HEADING_CHANGED, (payload) => {
      if (!payload) return
      const idx = payload.index
      // Defensive bound check: an in-flight event from a prior tab
      // could outlive the recompute that shrank the heading list.
      const headings = useOutlineStore.getState().headings
      if (idx !== null && (idx < 0 || idx >= headings.length)) return
      useOutlineStore.getState().setActiveIndex(idx)
    })

    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('outline', 'right')
      workspace.revealLeaf(leaf)
    })

    // Retain references so the tree-shaker doesn't strip the
    // subscription cleanups — they're owned by the plugin lifetime.
    void unsubscribeChanged
    void unsubscribeStore
  },
}
