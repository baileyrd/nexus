import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ActivityBar } from './ActivityBar'
import { useActivityBarStore, type ActivityBarItem } from './activityBarStore'

const EVENT_ITEM_ADDED = 'activityBar:itemAdded'
const EVENT_ITEM_REMOVED = 'activityBar:itemRemoved'
const EVENT_ACTIVE_CHANGED = 'activityBar:activeChanged'
const CONTEXT_KEY_ACTIVE = 'nexus.activityBar.activeView'

export const activityBarPlugin: Plugin = {
  manifest: {
    id: 'nexus.activityBar',
    name: 'Activity Bar',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      contextKeys: [
        {
          key: CONTEXT_KEY_ACTIVE,
          description: 'viewId of the currently selected activity-bar item, or empty when none.',
          type: 'string',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    const store = useActivityBarStore.getState()

    api.events.on<Omit<ActivityBarItem, never>>(EVENT_ITEM_ADDED, (payload) => {
      useActivityBarStore.getState().addItem(payload as ActivityBarItem)
    })

    api.events.on<{ id: string }>(EVENT_ITEM_REMOVED, ({ id }) => {
      const s = useActivityBarStore.getState()
      const removed = s.items.find((i) => i.id === id)
      s.removeItem(id)
      if (removed && removed.viewId === s.activeViewId) {
        s.setActive(null)
        api.context.set(CONTEXT_KEY_ACTIVE, '')
        api.events.emit(EVENT_ACTIVE_CHANGED, { viewId: null })
      }
    })

    // Post-Phase-7: every activity-bar item is expected to supply a
    // `command` (typically its plugin's focus command, which calls
    // `workspace.ensureLeafOfType + revealLeaf`). For legacy items
    // without a command we still mark them active so the icon
    // highlight stays coherent.
    const handleItemClick = (item: ActivityBarItem) => {
      if (item.command) {
        api.commands.execute(item.command)
      }
      const s = useActivityBarStore.getState()
      if (s.activeViewId === item.viewId) {
        s.setActive(null)
        api.context.set(CONTEXT_KEY_ACTIVE, '')
        api.events.emit(EVENT_ACTIVE_CHANGED, { viewId: null })
      } else {
        s.setActive(item.viewId)
        api.context.set(CONTEXT_KEY_ACTIVE, item.viewId)
        api.events.emit(EVENT_ACTIVE_CHANGED, { viewId: item.viewId })
      }
    }

    store.setActive(null)
    api.context.set(CONTEXT_KEY_ACTIVE, '')

    api.views.register('nexus.activityBar.view', {
      slot: 'activityBar',
      component: () => createElement(ActivityBar, { onItemClick: handleItemClick }),
      priority: 10,
    })
  },
}
