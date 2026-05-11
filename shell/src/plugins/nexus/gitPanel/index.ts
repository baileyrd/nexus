import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { useGitStatusStore } from '../gitStatus/gitStatusStore'
import { useGitPanelStore } from './gitPanelStore'
import { setGitPanelApi } from './gitPanelRuntime'
import { gitPanelViewCreator } from './GitPanelPaneView'

const GIT_ID             = 'com.nexus.git'
const VIEW_TYPE          = 'git-panel'
const VIEW_ID            = 'nexus.gitPanel.view'
const ACTIVITY_ITEM_ID   = 'nexus.gitPanel.activityItem'
const COMMAND_FOCUS      = 'nexus.gitPanel.focus'
const TOPIC_PREFIX       = 'com.nexus.git.'

export const gitPanelPlugin: Plugin = {
  manifest: {
    id: 'nexus.gitPanel',
    name: 'Git Panel',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.gitStatus'],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Git Panel', category: 'Git' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    setGitPanelApi(api)

    // Register the view type so workspace.ensureLeafOfType can create it.
    api.viewRegistry.register(VIEW_TYPE, gitPanelViewCreator())

    // ── Data loading ──────────────────────────────────────────────────
    const loadAll = async (): Promise<void> => {
      if (!(await api.kernel.available())) return
      const status = await api.kernel.invoke<{ branch: string | null; head: string; is_dirty: boolean; repo_state: string } | null>(
        GIT_ID, 'status', {},
      ).catch(() => null)
      useGitStatusStore.getState().setStatus(status)
      if (!status) return

      const s = useGitPanelStore.getState()
      const [files, branches, log, stash] = await Promise.all([
        api.kernel.invoke<Array<{ path: string; status: string }>>(GIT_ID, 'file_statuses', {}).catch(() => []),
        api.kernel.invoke<Array<{ name: string; is_head: boolean; upstream?: string }>>(GIT_ID, 'branches', {}).catch(() => []),
        api.kernel.invoke<Array<{ hash: string; author: string; date: string; message: string; parents: string[] }>>(GIT_ID, 'log', { limit: 50 }).catch(() => []),
        api.kernel.invoke<Array<{ index: number; message: string; oid: string }>>(GIT_ID, 'stash_list', {}).catch(() => []),
      ])
      s.setFiles(files)
      s.setBranches(branches)
      s.setLogEntries(log)
      s.setStashEntries(stash)
    }

    // Refresh on git events (branch change, commit, dirty toggle).
    let gitUnsubs: Array<() => void> = []

    const subscribeGitEvents = async () => {
      if (gitUnsubs.length > 0) return
      try {
        const unsub = await api.kernel.on(TOPIC_PREFIX, () => void loadAll())
        gitUnsubs = [unsub]
      } catch {
        gitUnsubs = []
      }
    }

    const unsubscribeGitEvents = () => {
      for (const unsub of gitUnsubs) { try { unsub() } catch { /* ignored */ } }
      gitUnsubs = []
    }

    api.events.on('workspace:opened', () => {
      void loadAll()
      void subscribeGitEvents()
    })
    api.events.on('workspace:closed', () => {
      useGitPanelStore.getState().reset()
      unsubscribeGitEvents()
    })

    if (await api.kernel.available()) {
      await loadAll()
      void subscribeGitEvents()
    }

    // ── Command ───────────────────────────────────────────────────────
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
    })

    // ── Activity bar ──────────────────────────────────────────────────
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconName: 'git',
      title: 'Source Control',
      viewId: VIEW_ID,
      priority: 25,
      command: COMMAND_FOCUS,
    })
  },
}
