import type { Plugin, PluginAPI } from '../../../types/plugin'
import { FileStats } from './FileStats'
import { IndexingStatus } from './IndexingStatus'

export const statusBarPlugin: Plugin = {
  manifest: {
    id: 'nexus.statusBar',
    name: 'Status Bar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    // `nexus.backlinks` is a soft dep — FileStats reads its zustand
    // store, which is safe to read with no provider (returns 0). Listing
    // it as a hard dep wedged status-bar activation when backlinks was
    // default-off.
    dependsOn: ['nexus.workspace', 'nexus.editor'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.views.register('nexus.statusBar.fileStats', {
      slot: 'statusBarRight',
      component: FileStats,
      priority: 10,
    })

    // BL-041 — background indexing daemon status badge. Polls
    // `com.nexus.ai::index_status` every 2 s; renders nothing when
    // the daemon has never run.
    api.views.register('nexus.statusBar.indexingStatus', {
      slot: 'statusBarRight',
      component: () => <IndexingStatus api={api} />,
      priority: 20,
    })
  },
}
