import type { Plugin, PluginAPI } from '../../../types/plugin'

const VIEW_ID = 'nexus.bookmarks.view'
const COMMAND_FOCUS = 'nexus.bookmarks.focus'
const EVENT_SIDEBAR_SHOW_VIEW = 'sidebar:showView'

function BookmarksView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--fg-dim)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will list saved bookmarks
      grouped by collection.
    </div>
  )
}

export const bookmarksPlugin: Plugin = {
  manifest: {
    id: 'nexus.bookmarks',
    name: 'Bookmarks',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Bookmarks', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.views.register(VIEW_ID, {
      slot: 'sidebarContent',
      component: BookmarksView,
      priority: 30,
    })

    api.activityBar.addItem({
      id: 'nexus.bookmarks.activityItem',
      icon: '',
      iconName: 'book',
      title: 'Bookmarks',
      viewId: VIEW_ID,
      priority: 30,
    })

    api.commands.register(COMMAND_FOCUS, () => {
      api.events.emit(EVENT_SIDEBAR_SHOW_VIEW, { viewId: VIEW_ID })
    })
  },
}
