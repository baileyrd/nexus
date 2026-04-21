import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { useRightPanelStore } from './rightPanelStore'

const EVENT_REGISTER_TAB = 'rightPanel:registerTab'
const EVENT_UNREGISTER_TAB = 'rightPanel:unregisterTab'
const COMMAND_TOGGLE = 'nexus.rightPanel.toggle'
const CONTEXT_KEY_VISIBLE = 'nexus.rightPanel.visible'

interface RegisterTabPayload {
  viewId: string
  title: string
  priority?: number
  iconName?: string
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
    // Phase 7: legacy SlotRegistry slot:'rightPanel' host removed —
    // the right sidedock is now rendered by <Workspace>. The tab
    // metadata store + toggle command remain so existing rightPanel-
    // aware plugins keep working.

    // Seed + track the visible context key from the workspace sidedock.
    const syncVisible = () => {
      api.context.set(CONTEXT_KEY_VISIBLE, !workspace.rightSplit.collapsed)
    }
    syncVisible()
    workspace.on('layout-change', syncVisible)

    // Tab metadata registration. Retained for legacy consumers.
    api.events.on<RegisterTabPayload>(EVENT_REGISTER_TAB, (payload) => {
      if (!payload || typeof payload.viewId !== 'string') return
      useRightPanelStore.getState().registerTab(payload.viewId, {
        title: payload.title ?? payload.viewId,
        priority: typeof payload.priority === 'number' ? payload.priority : 100,
        iconName: typeof payload.iconName === 'string' ? payload.iconName : undefined,
      })
    })

    api.events.on<UnregisterTabPayload>(EVENT_UNREGISTER_TAB, (payload) => {
      if (!payload || typeof payload.viewId !== 'string') return
      useRightPanelStore.getState().unregisterTab(payload.viewId)
    })

    api.commands.register(COMMAND_TOGGLE, () => {
      const current = workspace.rightSplit.collapsed
      workspace.setSidedockCollapsed('right', !current)
    })
  },
}
