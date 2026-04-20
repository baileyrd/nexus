import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLayoutStore } from '../../../stores/layoutStore'
import { useRightPanelStore } from '../rightPanel/rightPanelStore'

const VIEW_ID = 'nexus.outgoingLinks.view'
const COMMAND_FOCUS = 'nexus.outgoingLinks.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'

/** Placeholder body. Extraction of outgoing links from the active
 *  editor buffer is not yet implemented; the tab + command are
 *  scaffolded so the titlebar shortcut has a real target. */
function OutgoingLinksView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--fg-dim)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will list outgoing links
      from the active note once a forward-link extractor ships.
    </div>
  )
}

export const outgoingLinksPlugin: Plugin = {
  manifest: {
    id: 'nexus.outgoingLinks',
    name: 'Outgoing Links',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Outgoing Links', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.views.register(VIEW_ID, {
      slot: 'rightPanelContent',
      component: OutgoingLinksView,
      priority: 25,
    })
    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Outgoing',
      priority: 25,
    })
    api.commands.register(COMMAND_FOCUS, () => {
      useLayoutStore.setState((s) => ({
        rightPanel: { ...s.rightPanel, visible: true },
      }))
      useRightPanelStore.getState().setActive(VIEW_ID)
    })
  },
}
