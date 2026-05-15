// shell/src/plugins/nexus/notificationsInbox/index.ts
//
// BL-136 Phase 2 — Notification Center.
//
// Consumes the Phase 1 IPC surface on `com.nexus.notifications`
// (`inbox_list`, `inbox_mark_read`, `inbox_dismiss`, `inbox_stats`)
// and renders a sidebar leaf with:
//   - unread + total count in the header
//   - per-source filter chips
//   - click-row to mark read
//   - per-row "✓" (mark read) / "×" (dismiss) actions
//   - "Jump to run →" link for rows carrying a `task_id` payload —
//     fires the `nexus.aiRuntime.revealTask` command if registered,
//     otherwise emits the `nexus.notificationsInbox:jump-to-task`
//     event so a future BL-134 Phase 2 observability panel can wire
//     itself up without touching this plugin.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { clientLogger } from '../../../clientLogger'
import { NotificationsInboxView } from './NotificationsInboxView'
import {
  useNotificationsInboxStore,
  type InboxEntry,
} from './notificationsInboxStore'

const PLUGIN_ID = 'nexus.notificationsInbox'
const VIEW_ID = 'nexus.notificationsInbox.view'
const ACTIVITY_ITEM_ID = 'nexus.notificationsInbox.activityItem'
const COMMAND_SHOW = 'nexus.notificationsInbox.show'
const COMMAND_REFRESH = 'nexus.notificationsInbox.refresh'

const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const NOTIFICATIONS_PLUGIN_ID = 'com.nexus.notifications'
const TOPIC_INBOX_APPENDED = 'com.nexus.notifications.inbox.appended'

/** Forwarded to a future BL-134 Phase 2 observability panel so it can
 *  pick up the task_id without coupling to this plugin. */
const EVENT_JUMP_TO_TASK = 'nexus.notificationsInbox:jump-to-task'
const COMMAND_REVEAL_TASK = 'nexus.aiRuntime.revealTask'

/** Bell icon — stroke-only, matches the iconPath contract used by
 *  other activity-bar items. Lucide `bell`. */
const BELL_ICON_PATH =
  'M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0'

async function hydrate(api: PluginAPI): Promise<void> {
  try {
    const rows = await api.kernel.invoke<InboxEntry[]>(
      NOTIFICATIONS_PLUGIN_ID,
      'inbox_list',
      { limit: 200 },
    )
    useNotificationsInboxStore
      .getState()
      .hydrate(Array.isArray(rows) ? rows : [])
  } catch (err) {
    clientLogger.debug(
      '[nexus.notificationsInbox] inbox_list hydrate failed:',
      err,
    )
    // Mark hydrated even on failure so the panel renders an empty
    // state instead of an infinite "Loading…". The notifications
    // plugin may be disabled or the inbox may not be wired.
    useNotificationsInboxStore.getState().hydrate([])
  }
}

async function markRead(api: PluginAPI, ids: string[]): Promise<void> {
  if (ids.length === 0) return
  // Optimistic local update — the IPC reply doesn't carry per-id
  // detail and the live `inbox.appended` topic doesn't fire for
  // user-state mutations.
  useNotificationsInboxStore.getState().markRead(ids)
  try {
    await api.kernel.invoke(NOTIFICATIONS_PLUGIN_ID, 'inbox_mark_read', { ids })
  } catch (err) {
    clientLogger.warn(
      '[nexus.notificationsInbox] inbox_mark_read failed:',
      err,
    )
    // Resync from the source of truth — the optimistic flip lost.
    void hydrate(api)
  }
}

async function dismiss(api: PluginAPI, ids: string[]): Promise<void> {
  if (ids.length === 0) return
  useNotificationsInboxStore.getState().markDismissed(ids)
  try {
    await api.kernel.invoke(NOTIFICATIONS_PLUGIN_ID, 'inbox_dismiss', { ids })
  } catch (err) {
    clientLogger.warn('[nexus.notificationsInbox] inbox_dismiss failed:', err)
    void hydrate(api)
  }
}

function jumpToTask(api: PluginAPI, taskId: string): void {
  // Prefer the dedicated command if a Phase-2 observability panel
  // registered one; fall back to a generic event so any consumer can
  // listen.
  api.commands
    .execute(COMMAND_REVEAL_TASK, { taskId })
    .catch(() => {
      api.events.emit(EVENT_JUMP_TO_TASK, { taskId })
    })
}

export const notificationsInboxPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Notification Center',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.paneMode', 'nexus.activityBar'],
    contributes: {
      commands: [
        {
          id: COMMAND_SHOW,
          title: 'Show Notification Center',
          category: 'Notifications',
        },
        {
          id: COMMAND_REFRESH,
          title: 'Refresh Notification Inbox',
          category: 'Notifications',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    // ── View registration ─────────────────────────────────────────────
    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(NotificationsInboxView, {
          onMarkRead: (ids) => {
            void markRead(api, ids)
          },
          onDismiss: (ids) => {
            void dismiss(api, ids)
          },
          onJumpToTask: (taskId) => jumpToTask(api, taskId),
        }),
      priority: 10,
    })

    // ── Activity-bar item ─────────────────────────────────────────────
    // Priority 57 sits between the activity timeline (55) and
    // processes (60) so the trio of observability-style surfaces is
    // visually grouped.
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: BELL_ICON_PATH,
      title: 'Notifications',
      viewId: VIEW_ID,
      priority: 57,
    })

    // ── Activity-bar routing — same idiom as nexus.activityTimeline ───
    api.events.on<{ viewId: string | null }>(
      EVENT_ACTIVITY_BAR_ACTIVE_CHANGED,
      ({ viewId }) => {
        if (viewId === VIEW_ID) {
          void hydrate(api)
          void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
        } else {
          const current = usePaneModeStore.getState().activeViewId
          if (current === VIEW_ID) {
            void api.commands.execute(COMMAND_PANE_MODE_EXIT)
          }
        }
      },
    )

    // ── Commands ──────────────────────────────────────────────────────
    api.commands.register(COMMAND_SHOW, async () => {
      await hydrate(api)
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })
    api.commands.register(COMMAND_REFRESH, () => {
      void hydrate(api)
    })

    // ── Bus subscription ──────────────────────────────────────────────
    const kernelUnsubs: Array<() => void> = []

    const subscribe = async () => {
      if (kernelUnsubs.length > 0) return
      try {
        // The append topic carries only `{ id, source, severity, ts }`
        // — not the full row. Re-fetch on every append. inbox_list is
        // cheap (indexed by ts DESC, ~100 rows page) so the bus event
        // is the trigger and the IPC is the source of truth.
        const unsub = await api.kernel.on<unknown>(
          TOPIC_INBOX_APPENDED,
          (_topic, _payload) => {
            void hydrate(api)
          },
        )
        kernelUnsubs.push(unsub)
      } catch (err) {
        clientLogger.warn(
          '[nexus.notificationsInbox] subscribe failed:',
          err,
        )
      }
    }

    const unsubscribe = () => {
      while (kernelUnsubs.length > 0) {
        const unsub = kernelUnsubs.pop()
        if (!unsub) continue
        try {
          unsub()
        } catch (err) {
          clientLogger.warn(
            '[nexus.notificationsInbox] unsubscribe failed:',
            err,
          )
        }
      }
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void hydrate(api)
      void subscribe()
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      unsubscribe()
      // Leave the store populated — switching forges re-hydrates
      // with the new inbox. Clearing here would briefly flash an
      // empty pane on workspace switch.
    })

    // Cover the boot race: workspace:opened may have fired before our
    // listener attached.
    if (await api.kernel.available()) {
      void hydrate(api)
      void subscribe()
    }
  },
}
