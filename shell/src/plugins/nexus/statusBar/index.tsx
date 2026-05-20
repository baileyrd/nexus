import type { Plugin, PluginAPI } from '../../../types/plugin'
import { FileStats } from './FileStats'
import { IndexingStatus } from './IndexingStatus'
import { WorkspaceStatus } from './WorkspaceStatus'

export const statusBarPlugin: Plugin = {
  manifest: {
    id: 'nexus.statusBar',
    name: 'Status Bar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    // `nexus.backlinks` is a *soft* dep — deliberately omitted from
    // `dependsOn` so that the status bar still activates when the
    // user has backlinks disabled (default-off plugin). The FileStats
    // component reads the backlinks zustand store directly; if no
    // provider has registered, the store returns 0 and the badge
    // simply renders "0 references" rather than wedging activation.
    // If a future schema gains a `softDependsOn` field, add
    // `'nexus.backlinks'` there to make this intent declarative
    // without changing the activation semantics.
    dependsOn: ['nexus.workspace', 'nexus.editor'],
  },

  activate(api: PluginAPI) {
    // BL-053 Phase 1 — leftmost item in the status bar's right group:
    // forge name with an ember status dot when the workspace is in
    // sync. Mockup's bottom-right "lap-working · •" pattern.
    api.views.register('nexus.statusBar.workspace', {
      slot: 'statusBarRight',
      component: WorkspaceStatus,
      priority: 5,
    })

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
