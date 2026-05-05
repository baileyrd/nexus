/**
 * `nexus.agent` shell plugin — session-driven (ADR 0024 + 0025).
 *
 * Drives `com.nexus.agent::session_run` with `auto_approve: false`,
 * subscribes to the `com.nexus.agent.round_proposed` event the
 * core plugin emits whenever a round needs user approval, and
 * posts `round_decide` once the user clicks. Past sessions are
 * surfaced through `session_list` / `session_get` /
 * `session_delete`.
 *
 * The kernel-facing runtime lives in `agentRuntime.ts` so unit
 * tests can drive it without dragging the React view + CSS into
 * a node:test context.
 */

import { createElement } from 'react'

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { AgentSessionView } from './AgentSessionView'
import { createAgentRuntime } from './agentRuntime'
import { useAgentSessionStore } from './sessionStore'

export { AGENT_PLUGIN_ID, createAgentRuntime } from './agentRuntime'
export type { AgentRuntimeDeps } from './agentRuntime'

const PLUGIN_ID = 'nexus.agent'
const VIEW_ID = 'nexus.agent.view'
const ACTIVITY_ITEM_ID = 'nexus.agent.activityItem'

const COMMAND_SHOW = 'nexus.agent.show'
const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

export const agentPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Agent',
    version: '0.2.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.paneMode'],
    contributes: {
      commands: [{ id: COMMAND_SHOW, title: 'Show Agent', category: 'Agent' }],
    },
  },

  async activate(api: PluginAPI) {
    const runtime = createAgentRuntime(api)

    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(AgentSessionView, {
          onRun: () => void runtime.startSession(),
          onApprove: (decision, reason) => void runtime.submitDecision(decision, reason),
          onSelectSession: (id) => void runtime.selectSession(id),
          onDeleteSession: (id) => void runtime.deleteSession(id),
          onRefreshSessions: () => void runtime.refreshSessions(),
          onClearLive: runtime.clearLive,
        }),
      priority: 20,
    })

    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconName: 'sparkle',
      title: 'Agent',
      viewId: VIEW_ID,
      priority: 70,
    })

    api.commands.register(COMMAND_SHOW, async () => {
      void runtime.refreshSessions()
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })

    api.events.on<{ viewId: string | null }>(EVENT_ACTIVITY_BAR_ACTIVE_CHANGED, ({ viewId }) => {
      if (viewId === VIEW_ID) {
        void runtime.refreshSessions()
        void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
      } else {
        const current = usePaneModeStore.getState().activeViewId
        if (current === VIEW_ID) {
          void api.commands.execute(COMMAND_PANE_MODE_EXIT)
        }
      }
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void runtime.refreshSessions()
      void runtime.subscribeTopics()
      void runtime.loadArchetypes()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useAgentSessionStore.getState().reset()
      runtime.unsubscribeTopics()
    })

    if (await api.kernel.available()) {
      void runtime.refreshSessions()
      void runtime.subscribeTopics()
      void runtime.loadArchetypes()
    }
  },
}

export default agentPlugin
