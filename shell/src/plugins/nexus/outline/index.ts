import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { OutlineView } from './OutlineView'
import { outlinePaneViewCreator } from './OutlinePaneView'
import { useOutlineStore } from './outlineStore'
import { parseHeadings } from './parse'
import { useEditorStore } from '../editor/editorStore'

const VIEW_ID = 'nexus.outline.view'
const COMMAND_FOCUS = 'nexus.outline.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'
const EVENT_ACTIVE_HEADING_CHANGED = 'editor:activeHeadingChanged'

interface ActiveHeadingPayload {
  index: number | null
}

export const outlinePlugin: Plugin = {
  manifest: {
    id: 'nexus.outline',
    name: 'Outline',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Outline', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    // Phase 7: legacy SlotRegistry slot:'rightPanelContent' entry removed.
    viewRegistry.register(
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

    // Cross-plugin store import: read editor tabs directly. This is
    // the accepted shell pattern — titleBar / gitStatus similarly
    // read workspaceStore without going through the event bus.
    const recompute = () => {
      const s = useEditorStore.getState()
      const tab = s.tabs.find((t) => t.relpath === s.activeRelpath)
      if (!tab) {
        useOutlineStore.getState().clear()
        return
      }
      // Reset activeIndex with the headings: the editor will re-emit
      // a fresh `activeHeadingChanged` once its scroll-spy effect
      // re-runs against the new content. Leaving the old index in
      // place would briefly highlight the wrong row.
      useOutlineStore.getState().setHeadings(parseHeadings(tab.content))
      useOutlineStore.getState().setActiveIndex(null)
    }

    useEditorStore.subscribe((state, prev) => {
      if (state.activeRelpath !== prev.activeRelpath) {
        recompute()
        return
      }
      const a = state.tabs.find((t) => t.relpath === state.activeRelpath)
      const b = prev.tabs.find((t) => t.relpath === prev.activeRelpath)
      if (a?.content !== b?.content) recompute()
    })

    // Seed with whatever is active right now.
    recompute()

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
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
  },
}
