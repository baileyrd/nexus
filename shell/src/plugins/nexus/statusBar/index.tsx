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
    popoutCompatible: false,
    // `nexus.backlinks` is a soft dep — FileStats reads its zustand
    // store, which is safe to read with no provider (returns 0). Listing
    // it as a hard dep wedged status-bar activation when backlinks was
    // default-off.
    dependsOn: ['nexus.workspace', 'nexus.editor'],
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
      component: () => <IndexingStatus api={api} onReindex={() => triggerReindex(api)} />,
      priority: 20,
    })
  },
}

async function triggerReindex(api: PluginAPI): Promise<void> {
  try {
    const result = await api.kernel.invoke<{ queued?: number }>(
      'com.nexus.ai',
      'index_trigger',
      {},
    )
    const queued = typeof result?.queued === 'number' ? result.queued : 0
    api.notifications.show({
      type: 'info',
      message: queued > 0
        ? `Reindex queued: ${queued} file${queued === 1 ? '' : 's'}.`
        : 'Reindex queued: nothing to do (forge index empty).',
    })
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    api.notifications.show({ type: 'error', message: `Reindex failed: ${msg}` })
  }
}
