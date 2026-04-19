import type { Plugin, PluginAPI } from '../../../types/plugin'
import { invoke } from '@tauri-apps/api/core'
import { useGitStatusStore, type GitStatus } from './gitStatusStore'
import { GitStatusItem } from './GitStatusItem'

const EVENT_OPENED = 'workspace:opened'
const EVENT_CLOSED = 'workspace:closed'

async function loadStatus(path: string): Promise<void> {
  try {
    const status = await invoke<GitStatus | null>('get_git_status', { path })
    useGitStatusStore.getState().setStatus(status)
    console.info('[nexus.gitStatus] loaded for', path, status)
  } catch (err) {
    console.warn('[nexus.gitStatus] load failed for', path, err)
    useGitStatusStore.getState().setStatus(null)
  }
}

export const gitStatusPlugin: Plugin = {
  manifest: {
    id: 'nexus.gitStatus',
    name: 'Git Status',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    api.events.on<{ path: string }>(EVENT_OPENED, (payload) => {
      loadStatus(payload.path)
    })
    api.events.on(EVENT_CLOSED, () => {
      useGitStatusStore.getState().setStatus(null)
    })

    // If a workspace is already open (restored from persistence during
    // nexus.workspace activation, before we subscribed), load it now.
    const currentRoot = api.context.get('nexus.workspace.rootPath')
    if (typeof currentRoot === 'string' && currentRoot.length > 0) {
      await loadStatus(currentRoot)
    }

    api.views.register('nexus.gitStatus.item', {
      slot: 'statusBarLeft',
      component: GitStatusItem,
      priority: 20,
    })
  },
}
