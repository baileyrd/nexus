// Phase 4.3 — Note Context panel.
//
// Single right-panel view ("Note Context") containing an accordion of
// four sections that show different facets of the active markdown
// file: backlinks, outgoing links, tags, and a per-file graph view.
//
// As of step 6 (commit 24ae8958 series), the three sibling plugins
// `nexus.backlinks`, `nexus.outgoingLinks`, and `nexus.tags` are
// retired and absorbed here. Their plugin ids migrate via the
// `legacyPluginIds` field in catalog.ts so existing `plugins.enabled`
// lists carry forward into noteContext. Their focus-command ids
// (`nexus.<id>.focus`) are re-registered below as aliases that
// reveal the panel AND expand the matching section so muscle-memory
// keybindings and command-palette entries keep working.
//
// The standalone `nexus.graph` plugin is retained per the Phase 4.3
// product decision (some users dock the graph in its own tab) — its
// `GraphView` is reused by the Graph section here without
// duplicating the kernel subscription.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { NoteContextPaneView } from './NoteContextPaneView'
import { setEventBus } from './eventBus'
import { startBacklinksLoader } from './backlinksLoader'
import { useNoteContextStore } from './store'

const VIEW_TYPE = 'note-context'
const VIEW_ID = 'nexus.noteContext.view'
const COMMAND_FOCUS = 'nexus.noteContext.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'

// Legacy command ids → section id to expand. Registered as aliases
// in `activate()` below so the retired sibling plugins' focus
// commands keep working.
const LEGACY_FOCUS_ALIASES: Array<{ id: string; section: string }> = [
  { id: 'nexus.backlinks.focus',     section: 'backlinks' },
  { id: 'nexus.outgoingLinks.focus', section: 'outgoingLinks' },
  { id: 'nexus.tags.focus',          section: 'tags' },
]

/** Expand `id` in the accordion if it isn't already. Idempotent. */
function expandSection(id: string): void {
  const store = useNoteContextStore.getState()
  if (!store.isExpanded(id)) store.toggle(id)
}

async function focusPanelAndSection(section: string | null): Promise<void> {
  const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
  workspace.revealLeaf(leaf)
  if (section) expandSection(section)
}

export const noteContextPlugin: Plugin = {
  manifest: {
    id: 'nexus.noteContext',
    name: 'Note Context',
    version: '0.2.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    // nexus.graph owns the per-file graph data subscriber; the Graph
    // section in our accordion renders its `GraphView` component
    // directly and reads `useGraphStore`. nexus.editor and nexus.files
    // provide the stores + kernelClient that useActiveFileQuery
    // reaches into for the other three sections.
    dependsOn: [
      'nexus.rightPanel',
      'nexus.graph',
      'nexus.editor',
      'nexus.files',
      'com.nexus.storage',
    ],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Note Context', category: 'View' },
        // Legacy aliases — same title strings the retired plugins used
        // so the command palette doesn't lose entries.
        { id: 'nexus.backlinks.focus',     title: 'Focus Backlinks',      category: 'View' },
        { id: 'nexus.outgoingLinks.focus', title: 'Focus Outgoing Links', category: 'View' },
        { id: 'nexus.tags.focus',          title: 'Focus Tags',           category: 'View' },
      ],
    },
  },

  activate(api: PluginAPI) {
    setEventBus(api.events)

    // Always-on backlinks subscriber — populates useBacklinksDataStore
    // so the RightPanelFooter / FileStats indicators stay fresh
    // regardless of whether the Backlinks accordion section is
    // currently expanded. See backlinksLoader.ts for the trade-off.
    startBacklinksLoader(api)

    api.viewRegistry.register(VIEW_TYPE, (leaf) => new NoteContextPaneView(leaf))

    // Advertise the tab to the right-panel host.
    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Note Context',
      priority: 10,
      iconName: 'sidebarRight',
    })

    api.commands.register(COMMAND_FOCUS, () => focusPanelAndSection(null))

    for (const { id, section } of LEGACY_FOCUS_ALIASES) {
      api.commands.register(id, () => focusPanelAndSection(section))
    }
  },
}
