import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { SearchView } from './SearchView'
import { searchPaneViewCreator } from './SearchPaneView'
import { useSearchStore, type SearchHit } from './searchStore'
import {
  cancelInFlight,
  requestFocus,
  setKernel,
} from './searchRuntime'

const COMMAND_FOCUS = 'nexus.search.focus'

const EVENT_FILE_OPEN = 'files:open'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

export const searchPlugin: Plugin = {
  manifest: {
    id: 'nexus.search',
    name: 'Search',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      configuration: {
        pluginId: 'nexus.search',
        title: 'Search',
        order: 30,
        category: 'navigation',
        schema: [
          {
            key: 'search.maxResultsLimit',
            title: 'Max search results',
            description: 'Maximum number of results returned by a workspace search query',
            type: 'number' as const,
            default: 50,
          },
        ],
      },
      commands: [
        {
          id: COMMAND_FOCUS,
          title: 'Focus Search',
          category: 'Search',
        },
      ],
      keybindings: [
        { command: COMMAND_FOCUS, key: 'ctrl+shift+f', mac: 'cmd+shift+f' },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.configuration.register(searchPlugin.manifest.contributes!.configuration!)
    setKernel(api.kernel)

    const handleHitActivate = (hit: SearchHit) => {
      // Mirror the event nexus.files emits. The editor already
      // subscribes to `files:open`, so a click on a search result
      // opens a tab exactly like a tree click.
      api.events.emit(EVENT_FILE_OPEN, {
        relpath: hit.relpath,
        name: basename(hit.relpath) || hit.relpath,
      })
    }

    // Phase 7: legacy SlotRegistry slot:'sidebarContent' registration
    // removed. The view is now hosted exclusively via the Leaf/View
    // pipeline below.
    api.viewRegistry.register(
      'search',
      searchPaneViewCreator(() =>
        createElement(SearchView, { onHitActivate: handleHitActivate }),
      ),
    )

    // Search view is reached via the sidebar tab strip's search icon
    // (rendered by WorkspaceRenderer for sidedock leaves). No separate
    // activity-bar entry.

    // Focus command — raises the view and focuses the input.
    //
    // The view may or may not already be mounted. Cases:
    //   * View is active & mounted: focus() runs immediately via
    //     the registered focuser.
    //   * View is registered but sidebar is hidden / showing a
    //     different view: `ensureLeafOfType + revealLeaf` flips the
    //     sidedock to us; SearchView mounts; its mount effect drains
    //     the pending focus flag set by requestFocus() below.
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('search', 'left')
      workspace.revealLeaf(leaf)
      requestFocus()
    })

    // Wipe query + results + any in-flight call on workspace close.
    // The kernel is torn down during that window; letting a stale
    // request land would surface a confusing error in the sidebar.
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      cancelInFlight()
      useSearchStore.getState().reset()
    })
  },
}
