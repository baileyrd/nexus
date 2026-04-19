import type { Plugin, PluginAPI } from '../../../types/plugin'
import { TitleBar } from './TitleBar'

export const titleBarPlugin: Plugin = {
  manifest: {
    id: 'nexus.titleBar',
    name: 'Title Bar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.views.register('nexus.titleBar.view', {
      slot: 'titleBar',
      component: TitleBar,
      priority: 10,
    })
  },
}
