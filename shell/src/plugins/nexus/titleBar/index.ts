import type { Plugin, PluginAPI } from '../../../types/plugin'
import { TitleBar } from './TitleBar'
import { setApi } from './runtime'

export const titleBarPlugin: Plugin = {
  manifest: {
    id: 'nexus.titleBar',
    name: 'Title Bar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    // The breadcrumb reads workspace + editor active tab; the search
    // button dispatches a command from nexus.search. None of those
    // are blocking deps — workspace boots first regardless and the
    // search dispatch silently no-ops if it isn't loaded — but we
    // declare them so the load order is sensible.
    dependsOn: ['nexus.workspace'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    setApi(api)
    api.views.register('nexus.titleBar.view', {
      slot: 'titleBar',
      component: TitleBar,
      priority: 10,
    })
  },
}
