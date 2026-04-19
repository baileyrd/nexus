import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useGitStatusStore, type GitStatus } from './gitStatusStore'
import { GitStatusItem } from './GitStatusItem'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const GIT_PLUGIN_ID = 'com.nexus.git'
// Every custom event the kernel git plugin publishes is namespaced under
// `com.nexus.git.*` — one prefix subscription covers `state`,
// `branch_changed`, `commit`, and `dirty_changed`. Verified against
// crates/nexus-git/src/core_plugin.rs::publish_changes.
const TOPIC_PREFIX = 'com.nexus.git.'

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
    // Pull fresh state from the kernel. The git plugin tracks which repo
    // to query internally (its `forge_root` is fixed at on_init), so the
    // args object is empty. Returns `null` on the store if the workspace
    // isn't a git repo — the plugin surfaces that as a
    // `CommandNotFound`-style failure because `on_init` doesn't spawn a
    // worker, so `dispatch` returns `ExecutionFailed`. Either way we
    // swallow and clear.
    const loadStatus = async (): Promise<void> => {
      try {
        const status = await api.kernel.invoke<GitStatus>(
          GIT_PLUGIN_ID,
          'status',
          {},
        )
        useGitStatusStore.getState().setStatus(status)
        console.info('[nexus.gitStatus] loaded', status)
      } catch (err) {
        // Most common reason: forge root isn't a git repo (plugin returns
        // an ExecutionFailed on dispatch). Not an error worth surfacing
        // — just clear the UI.
        console.info('[nexus.gitStatus] unavailable:', err)
        useGitStatusStore.getState().setStatus(null)
      }
    }

    // ── Live refresh on git events ─────────────────────────────────────
    //
    // The git plugin runs a 2-second poller that diffs consecutive
    // `GitState` snapshots and publishes one of
    //   com.nexus.git.state            (initial snapshot)
    //   com.nexus.git.branch_changed   (branch shorthand flipped)
    //   com.nexus.git.commit           (HEAD oid flipped)
    //   com.nexus.git.dirty_changed    (working-tree dirty flag toggled)
    // on each tick when state has changed. The payloads are partial
    // (branch_changed omits is_dirty; commit omits is_dirty) so rather
    // than decoding each variant we just re-invoke `status` — it's the
    // canonical source of truth and one extra sync IPC call per real
    // transition is cheap.
    //
    // Subscription lifecycle mirrors nexus.files: drop the unsubscribe
    // handle on workspace:closed (kernel shuts down), re-subscribe on
    // workspace:opened.

    const handleGitEvent = (_topic: string, _payload: unknown) => {
      void loadStatus()
    }

    let gitUnsubs: Array<() => void> = []

    const subscribeGitEvents = async () => {
      if (gitUnsubs.length > 0) return
      try {
        const unsub = await api.kernel.on(TOPIC_PREFIX, handleGitEvent)
        gitUnsubs = [unsub]
      } catch (err) {
        console.warn('[nexus.gitStatus] failed to subscribe to git events:', err)
        gitUnsubs = []
      }
    }

    const unsubscribeGitEvents = () => {
      for (const unsub of gitUnsubs) {
        try {
          unsub()
        } catch (err) {
          console.warn('[nexus.gitStatus] unsubscribe failed:', err)
        }
      }
      gitUnsubs = []
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void loadStatus()
      void subscribeGitEvents()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useGitStatusStore.getState().setStatus(null)
      unsubscribeGitEvents()
    })

    // Workspace restoration happens synchronously inside
    // nexus.workspace.activate and emits `workspace:opened` before this
    // plugin's listener is registered on first boot. Cover that race by
    // pulling state + subscribing immediately if the kernel is already
    // up. Same pattern as nexus.files.
    if (await api.kernel.available()) {
      await loadStatus()
      void subscribeGitEvents()
    }

    api.views.register('nexus.gitStatus.item', {
      slot: 'statusBarLeft',
      component: GitStatusItem,
      priority: 20,
    })
  },
}
