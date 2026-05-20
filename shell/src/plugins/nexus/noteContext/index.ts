// Phase 4.3 — Note Context panel.
//
// Single right-panel view ("Note Context") containing an accordion of
// four sections that show different facets of the active markdown
// file: backlinks, outgoing links, tags, and a per-file graph view.
//
// Replaces (over the course of Phase 4.3) the four standalone
// sibling plugins that previously each registered its own right-panel
// tab. The standalone `nexus.graph` plugin is retained per the Phase
// 4.3 product decision (some users dock the graph separately); the
// other three plugins (`nexus.backlinks`, `nexus.outgoingLinks`,
// `nexus.tags`) are retired in step 6.
//
// Step 1: skeleton only — accordion shell + placeholder section
// bodies, registered alongside the four legacy plugins. Subsequent
// steps swap each placeholder for the real section content.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { NoteContextPaneView } from './NoteContextPaneView'
import { useNoteContextStore } from './store'

const VIEW_TYPE = 'note-context'
const VIEW_ID = 'nexus.noteContext.view'
const COMMAND_FOCUS = 'nexus.noteContext.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'

/**
 * Expose store helpers for the focus-command aliases that subsequent
 * steps register on behalf of the retired legacy plugins. Each alias
 * focuses the panel AND expands the matching section so muscle
 * memory survives.
 */
export function expandSection(id: string): void {
  const store = useNoteContextStore.getState()
  if (!store.isExpanded(id)) store.toggle(id)
}

export const noteContextPlugin: Plugin = {
  manifest: {
    id: 'nexus.noteContext',
    name: 'Note Context',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Note Context', category: 'View' },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new NoteContextPaneView(leaf))

    // Advertise the tab to the right-panel host.
    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Note Context',
      priority: 10,
      iconName: 'sidebarRight',
    })

    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
