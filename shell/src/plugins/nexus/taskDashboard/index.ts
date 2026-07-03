// shell/src/plugins/nexus/taskDashboard/index.ts
//
// C7 (#360) — task dashboard: the checkbox-task pipeline was fully
// plumbed server-side (parse at index time, SQLite tasks table,
// query_tasks/toggle_task IPC with file write-back) but the desktop
// shell — the primary frontend — had zero consumers of it. This plugin
// closes that gap with a pane-mode view grouped by due date
// (overdue/today/upcoming/no-date), click-to-toggle via the existing
// toggle_task write-back, and click-to-open the source file.
//
// Structure mirrors nexus.activity (activityTimeline/index.ts): a
// paneMode view + activity-bar item, hydrate-on-open from the storage
// IPC. Unlike activity, tasks have no live-append bus topic to
// subscribe to, so freshness comes from a full re-fetch whenever any
// file saves (`files:saved`) rather than an incremental stream.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { TaskDashboardView } from './TaskDashboardView'
import { useTaskDashboardStore } from './taskDashboardStore'
import { decodeTasks } from './taskGrouping'
import { setApi } from './taskDashboardRuntime'

const PLUGIN_ID = 'nexus.taskDashboard'
const VIEW_ID = 'nexus.taskDashboard.view'
const ACTIVITY_ITEM_ID = 'nexus.taskDashboard.activityItem'
const COMMAND_SHOW = 'nexus.taskDashboard.show'

const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'
const EVENT_FILE_SAVED = 'files:saved'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'

/** Lucide "square-check-big" glyph — a checkbox with a checkmark. */
const TASKS_ICON_PATH =
  'M21 10.656V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h9.5M9 11l3 3L22 4'

async function hydrate(api: PluginAPI): Promise<void> {
  try {
    const raw = await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'query_tasks', {})
    useTaskDashboardStore.getState().hydrate(decodeTasks(raw))
  } catch {
    // Fail open to an empty (but hydrated) dashboard — same shape as
    // activityTimeline's hydrate-on-error handling — rather than
    // leaving the pane stuck on "Loading…".
    useTaskDashboardStore.getState().hydrate([])
  }
}

export const taskDashboardPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Tasks',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.paneMode', 'nexus.activityBar'],
    contributes: {
      commands: [{ id: COMMAND_SHOW, title: 'Show Tasks', category: 'Tasks' }],
    },
  },

  async activate(api: PluginAPI) {
    setApi(api)

    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () => createElement(TaskDashboardView),
      priority: 10,
    })

    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: TASKS_ICON_PATH,
      title: 'Tasks',
      viewId: VIEW_ID,
      priority: 58,
    })

    api.events.on<{ viewId: string | null }>(EVENT_ACTIVITY_BAR_ACTIVE_CHANGED, ({ viewId }) => {
      if (viewId === VIEW_ID) {
        void hydrate(api)
        void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
      } else if (usePaneModeStore.getState().activeViewId === VIEW_ID) {
        void api.commands.execute(COMMAND_PANE_MODE_EXIT)
      }
    })

    api.commands.register(COMMAND_SHOW, async () => {
      await hydrate(api)
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })

    // Re-fetch on save rather than a live-append topic — the tasks
    // table has no per-change bus event today, and a whole-file
    // checkbox edit can add/remove/reorder several tasks at once, so
    // a full query_tasks re-run is simpler and correct where a
    // targeted patch would need to reconstruct diffs it doesn't have.
    api.events.on<{ relpath: string }>(EVENT_FILE_SAVED, () => {
      if (useTaskDashboardStore.getState().hydrated) {
        void hydrate(api)
      }
    })

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void hydrate(api)
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      // Leave the store populated — matches nexus.activity's rationale
      // (avoids a flash of empty state on tab-switch); the next
      // workspace re-hydrates on open.
    })

    if (await api.kernel.available()) {
      void hydrate(api)
    }
  },
}
