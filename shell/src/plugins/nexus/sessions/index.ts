/**
 * `nexus.sessions` shell plugin — the session-tree navigator (RFC 0008,
 * Phase 5.4).
 *
 * Renders stored agent sessions as a forest (resume / branch / rewind create
 * child nodes linked by `parent_id` / `branch_point`) and drives those fork
 * verbs + named checkpoints, all through `com.nexus.agent` IPC. Complements the
 * run-focused `nexus.agent` view: this one is for navigating + re-running what
 * already happened.
 *
 * The kernel-facing runtime lives in `sessionsRuntime.ts` so unit tests can
 * drive it without dragging the React view + CSS into a node:test context.
 */

import { createElement } from 'react'

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { SessionTreeView } from './SessionTreeView'
import { createSessionsRuntime } from './sessionsRuntime'
import { useSessionsStore } from './sessionsStore'

export { createSessionsRuntime } from './sessionsRuntime'
export type { SessionsRuntimeDeps } from './sessionsRuntime'

const PLUGIN_ID = 'nexus.sessions'
const VIEW_ID = 'nexus.sessions.view'
const ACTIVITY_ITEM_ID = 'nexus.sessions.activityItem'

const COMMAND_SHOW = 'nexus.sessions.show'
const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

export const sessionsPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Session Tree',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.paneMode'],
    contributes: {
      commands: [{ id: COMMAND_SHOW, title: 'Show Session Tree', category: 'Agent' }],
    },
  },

  async activate(api: PluginAPI) {
    const runtime = createSessionsRuntime(api)

    const refreshAll = (): void => {
      void runtime.refreshSessions()
      void runtime.refreshCheckpoints()
    }

    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(SessionTreeView, {
          onRefresh: refreshAll,
          onSelect: (id) => void runtime.selectSession(id),
          onResume: (id, message) => void runtime.resume(id, message),
          onBranch: (id, round, message) => void runtime.branch(id, round, message),
          onRewind: (id, round, message) => void runtime.rewind(id, round, message),
          onCheckpoint: (id, round, name) => void runtime.checkpoint(id, round, name),
          onDeleteCheckpoint: (name) => void runtime.deleteCheckpoint(name),
          onDeleteSession: (id) => void runtime.deleteSession(id),
        }),
      priority: 21,
    })

    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconName: 'git',
      title: 'Session Tree',
      viewId: VIEW_ID,
      priority: 69,
    })

    api.commands.register(COMMAND_SHOW, async () => {
      refreshAll()
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })

    api.events.on<{ viewId: string | null }>(EVENT_ACTIVITY_BAR_ACTIVE_CHANGED, ({ viewId }) => {
      if (viewId === VIEW_ID) {
        refreshAll()
        void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
      } else {
        const current = usePaneModeStore.getState().activeViewId
        if (current === VIEW_ID) {
          void api.commands.execute(COMMAND_PANE_MODE_EXIT)
        }
      }
    })

    api.events.on(EVENT_WORKSPACE_OPENED, refreshAll)
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useSessionsStore.getState().reset()
    })

    if (await api.kernel.available()) {
      refreshAll()
    }
  },
}

export default sessionsPlugin
