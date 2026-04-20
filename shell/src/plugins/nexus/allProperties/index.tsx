import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useLayoutStore } from '../../../stores/layoutStore'
import { useRightPanelStore } from '../rightPanel/rightPanelStore'

const VIEW_ID = 'nexus.allProperties.view'
const COMMAND_FOCUS = 'nexus.allProperties.focus'
const EVENT_REGISTER_TAB = 'rightPanel:registerTab'

/** Placeholder body. A frontmatter-properties inspector — table of
 *  every key/value on the active note plus inherited values — is not
 *  yet implemented. Tab + command exist so the titlebar shortcut
 *  resolves. */
function AllPropertiesView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--fg-dim)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will list every frontmatter
      property on the active note, including inherited values.
    </div>
  )
}

export const allPropertiesPlugin: Plugin = {
  manifest: {
    id: 'nexus.allProperties',
    name: 'All Properties',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.rightPanel'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus All Properties', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.views.register(VIEW_ID, {
      slot: 'rightPanelContent',
      component: AllPropertiesView,
      priority: 35,
    })
    api.events.emit(EVENT_REGISTER_TAB, {
      viewId: VIEW_ID,
      title: 'Properties',
      priority: 35,
    })
    api.commands.register(COMMAND_FOCUS, () => {
      useLayoutStore.setState((s) => ({
        rightPanel: { ...s.rightPanel, visible: true },
      }))
      useRightPanelStore.getState().setActive(VIEW_ID)
    })
  },
}
