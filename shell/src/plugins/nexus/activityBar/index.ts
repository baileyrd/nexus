import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ActivityBar } from './ActivityBar'
import { useActivityBarStore, type ActivityBarItem } from './activityBarStore'

const EVENT_ITEM_ADDED = 'activityBar:itemAdded'
const EVENT_ITEM_REMOVED = 'activityBar:itemRemoved'
const EVENT_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_SIDEBAR_SHOW_VIEW = 'sidebar:showView'
const EVENT_SIDEBAR_HIDE = 'sidebar:hide'
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
        api.events.emit(EVENT_SIDEBAR_HIDE, {})
        api.events.emit(EVENT_ACTIVE_CHANGED, { viewId: null })
      }
    })

    const handleItemClick = (item: ActivityBarItem) => {
      const s = useActivityBarStore.getState()
      if (s.activeViewId === item.viewId) {
        // Toggle off — deactivate and hide sidebar.
        s.setActive(null)
        api.context.set(CONTEXT_KEY_ACTIVE, '')
        api.events.emit(EVENT_SIDEBAR_HIDE, {})
        api.events.emit(EVENT_ACTIVE_CHANGED, { viewId: null })
      } else {
        s.setActive(item.viewId)
        api.context.set(CONTEXT_KEY_ACTIVE, item.viewId)
        api.events.emit(EVENT_SIDEBAR_SHOW_VIEW, { viewId: item.viewId })
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
