import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLayoutStore } from '../../../stores/layoutStore'
import { RightPanelHost } from './RightPanelHost'
import { useRightPanelStore } from './rightPanelStore'

const VIEW_ID = 'nexus.rightPanel.host'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'
const EVENT_UNREGISTER_TAB = 'rightPanel:unregisterTab'
const COMMAND_TOGGLE = 'nexus.rightPanel.toggle'
const CONTEXT_KEY_VISIBLE = 'nexus.rightPanel.visible'

interface RegisterTabPayload {
  viewId: string
  title: string
  priority?: number
}

interface UnregisterTabPayload {
  viewId: string
}

export const rightPanelPlugin: Plugin = {
  manifest: {
    id: 'nexus.rightPanel',
    name: 'Right Panel',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: COMMAND_TOGGLE, title: 'Toggle Right Panel', category: 'View' },
      ],
      keybindings: [
        { command: COMMAND_TOGGLE, key: 'ctrl+alt+r', mac: 'cmd+alt+r' },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_VISIBLE,
          description: 'True when the right panel is visible.',
          type: 'boolean',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    // Host view into the rightPanel slot. It will be rendered whenever
    // layoutStore.rightPanel.visible is true.
    api.views.register(VIEW_ID, {
      slot: 'rightPanel',
      component: RightPanelHost,
      priority: 10,
    })

    // Flip the strip visible on activate so the host shows up the
    // moment the plugin loads. main.tsx no longer forces it hidden.
    useLayoutStore.setState((s) => ({
      rightPanel: { ...s.rightPanel, visible: true },
    }))

    // Seed + track the visible context key.
    api.context.set(CONTEXT_KEY_VISIBLE, useLayoutStore.getState().rightPanel.visible)
    useLayoutStore.subscribe((state, prev) => {
      if (state.rightPanel.visible !== prev.rightPanel.visible) {
        api.context.set(CONTEXT_KEY_VISIBLE, state.rightPanel.visible)
      }
    })

    // Tab metadata registration. Fires in tandem with each
    // contributor's api.views.register call for rightPanelContent.
    api.events.on<RegisterTabPayload>(EVENT_REGISTER_TAB, (payload) => {
      if (!payload || typeof payload.viewId !== 'string') return
      useRightPanelStore.getState().registerTab(payload.viewId, {
        title: payload.title ?? payload.viewId,
        priority: typeof payload.priority === 'number' ? payload.priority : 100,
      })
    })

    api.events.on<UnregisterTabPayload>(EVENT_UNREGISTER_TAB, (payload) => {
      if (!payload || typeof payload.viewId !== 'string') return
      useRightPanelStore.getState().unregisterTab(payload.viewId)
    })

    api.commands.register(COMMAND_TOGGLE, async () => {
      useLayoutStore.getState().toggleRightPanel()
    })
  },
}
