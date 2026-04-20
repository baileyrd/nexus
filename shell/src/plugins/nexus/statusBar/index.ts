import type { Plugin, PluginAPI } from '../../../types/plugin'
import { FileStats } from './FileStats'

export const statusBarPlugin: Plugin = {
  manifest: {
    id: 'nexus.statusBar',
    name: 'Status Bar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.editor', 'nexus.backlinks'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.views.register('nexus.statusBar.fileStats', {
      slot: 'statusBarRight',
      component: FileStats,
      priority: 10,
    })
  },
}
