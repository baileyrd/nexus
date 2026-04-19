import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { SearchView } from './SearchView'
import { useSearchStore, type SearchHit } from './searchStore'
import {
  cancelInFlight,
  requestFocus,
  setKernel,
} from './searchRuntime'

const VIEW_ID = 'nexus.search.view'
const COMMAND_FOCUS = 'nexus.search.focus'

const EVENT_FILE_OPEN = 'files:open'
const EVENT_SIDEBAR_SHOW_VIEW = 'sidebar:showView'
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

    api.views.register(VIEW_ID, {
      slot: 'sidebarContent',
      component: () => createElement(SearchView, { onHitActivate: handleHitActivate }),
      priority: 20,
    })

    api.activityBar.addItem({
      id: 'nexus.search.activityItem',
      icon: '',
      iconName: 'search',
      title: 'Search',
      viewId: VIEW_ID,
      priority: 20,
    })

    // Focus command — raises the view and focuses the input.
    //
    // The view may or may not already be mounted. Cases:
    //   * View is active & mounted: focus() runs immediately via
    //     the registered focuser.
    //   * View is registered but sidebar is hidden / showing a
    //     different view: `sidebar:showView` flips the host to us;
    //     SearchView mounts; its mount effect drains the pending
    //     focus flag set by requestFocus() below.
    api.commands.register(COMMAND_FOCUS, () => {
      api.events.emit(EVENT_SIDEBAR_SHOW_VIEW, { viewId: VIEW_ID })
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
