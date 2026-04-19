import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ActivityBarView } from './ActivityBarView'
import { useActivityBarStore } from './activityBarStore'

export { useActivityBarStore } from './activityBarStore'
export type { ActivityBarItem } from './activityBarStore'

export const activityBarPlugin: Plugin = {
  manifest: {
    id: 'core.activity-bar',
    name: 'Activity Bar',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        { id: 'activityBar.toggle', title: 'Toggle Activity Bar' },
      ],
    },
  },
  activate(api: PluginAPI) {
    api.views.register('activityBar', {
      slot: 'activityBar',
      component: ActivityBarView,
      priority: 0,
    })

    api.events.on('activityBar:itemAdded', (config: unknown) => {
      const c = config as any
      useActivityBarStore.getState().addItem(c)
    })

    api.events.on('activityBar:itemRemoved', ({ id }: { id: string }) => {
      useActivityBarStore.getState().removeItem(id)
    })

    // Seed Forge-style placeholder rail items. Real feature plugins can
    // override any of these by re-registering with the same id; the store
    // dedupes on id. Priority leaves space between seeds so plugins can
    // slot themselves in (e.g., fileExplorer uses priority 10).
    const seeds = [
      { id: 'rail.search',    icon: 'search',   title: 'Search',    viewId: 'search',    priority: 20 },
      { id: 'rail.graph',     icon: 'graph',    title: 'Graph',     viewId: 'graph',     priority: 30 },
      { id: 'rail.tasks',     icon: 'task',     title: 'Tasks',     viewId: 'tasks',     priority: 40 },
      { id: 'rail.git',       icon: 'git',      title: 'Git',       viewId: 'git',       priority: 50 },
      { id: 'rail.db',        icon: 'db',       title: 'Bases',     viewId: 'db',        priority: 60 },
      { id: 'rail.templates', icon: 'star',     title: 'Templates', viewId: 'templates', priority: 70 },
      { id: 'rail.ai',        icon: 'sparkle',  title: 'AI',        viewId: 'ai',        priority: 80 },
    ]
    for (const s of seeds) api.activityBar.addItem(s)
  },
}
