import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLayoutStore } from '../../../stores/layoutStore'
import { useRightPanelStore } from '../rightPanel/rightPanelStore'

const VIEW_ID = 'nexus.tags.view'
const COMMAND_FOCUS = 'nexus.tags.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'

/** Placeholder body. A tag inspector — listing the active note's
 *  tags and offering filters into the global tag index — is not yet
 *  implemented. Tab + command exist so the titlebar shortcut resolves. */
function TagsView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--fg-dim)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will surface the active
      note's tags and their usage across the workspace.
    </div>
  )
}

export const tagsPlugin: Plugin = {
  manifest: {
    id: 'nexus.tags',
    name: 'Tags',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Tags', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.views.register(VIEW_ID, {
      slot: 'rightPanelContent',
      component: TagsView,
      priority: 30,
    })
    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Tags',
      priority: 30,
      iconName: 'tag',
    })
    api.commands.register(COMMAND_FOCUS, () => {
      useLayoutStore.setState((s) => ({
        rightPanel: { ...s.rightPanel, visible: true },
      }))
      useRightPanelStore.getState().setActive(VIEW_ID)
    })
  },
}
