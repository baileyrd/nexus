// shell/src/plugins/nexus/activityTimeline/index.ts
//
// BL-037 — AI activity timeline pane.
//
// Per-forge log of AI interactions (prompt, model, surface, files
// touched, tool calls, outcome). Hosted as a pane-mode view (same
// host pattern as `nexus.processes`) and an activity-bar entry.
//
// Data flow:
//
//   1. On activate, hydrate the store from
//      `com.nexus.ai::activity_list`. The kernel returns an empty
//      list when the recorder isn't wired (e.g. AI plugin disabled),
//      so a fresh forge keeps an empty pane without errors.
//   2. Subscribe to `com.nexus.ai.activity_appended`. The kernel
//      publishes the freshly-recorded `ActivityEntry` after every
//      successful append (chat / ask / cmdi / ghost / complete /
//      enrich), and the store prepends it.
//   3. The "Clear" button calls `com.nexus.ai::activity_clear` and
//      empties the local store. The on-disk JSONL is truncated.
//
// The pane sits behind a default-off plugin entry in the catalog —
// the timeline only matters when the user has the AI surface
// configured, which mirrors how `nexus.recall` / `nexus.memory` are
// gated.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { ActivityTimelineView } from './ActivityTimelineView'
import {
  useActivityTimelineStore,
  type ActivityEntry,
} from './activityTimelineStore'

const PLUGIN_ID = 'nexus.activityTimeline'
const VIEW_ID = 'nexus.activityTimeline.view'
const ACTIVITY_ITEM_ID = 'nexus.activityTimeline.activityItem'
const COMMAND_SHOW = 'nexus.activityTimeline.show'
const COMMAND_CLEAR = 'nexus.activityTimeline.clear'

const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const AI_PLUGIN_ID = 'com.nexus.ai'
const TOPIC_ACTIVITY_APPENDED = 'com.nexus.ai.activity_appended'

/**
 * Lucide-style "history / timeline" glyph — clock with a backwards
 * arrow. Stroke-only to match the iconPath contract used by other
 * activity-bar items.
 */
const TIMELINE_ICON_PATH =
  'M3 12a9 9 0 1 0 3-6.7M3 4v5h5M12 7v5l3 2'

/** Server-side response shape from `com.nexus.ai::activity_list`. */
interface ActivityListResult {
  entries: ActivityEntry[]
}

async function hydrateFromKernel(api: PluginAPI): Promise<void> {
  try {
    const result = await api.kernel.invoke<ActivityListResult>(
      AI_PLUGIN_ID,
      'activity_list',
    )
    useActivityTimelineStore
      .getState()
      .hydrate(Array.isArray(result?.entries) ? result.entries : [])
  } catch (err) {
    console.debug(
      '[nexus.activityTimeline] activity_list hydrate failed:',
      err,
    )
    // Mark hydrated even on failure so the empty-state replaces the
    // "Loading…" placeholder. Most likely cause: AI plugin disabled.
    useActivityTimelineStore.getState().hydrate([])
  }
}

async function clearTimeline(api: PluginAPI): Promise<void> {
  try {
    await api.kernel.invoke(AI_PLUGIN_ID, 'activity_clear')
  } catch (err) {
    console.warn(
      '[nexus.activityTimeline] activity_clear failed:',
      err,
    )
    return
  }
  useActivityTimelineStore.getState().clear()
}

export const activityTimelinePlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'AI Activity Timeline',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.paneMode', 'nexus.activityBar'],
    contributes: {
      commands: [
        {
          id: COMMAND_SHOW,
          title: 'Show AI Activity Timeline',
          category: 'AI',
        },
        {
          id: COMMAND_CLEAR,
          title: 'Clear AI Activity Timeline',
          category: 'AI',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    // ── View registration ─────────────────────────────────────────────
    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(ActivityTimelineView, {
          onClear: () => {
            void clearTimeline(api)
          },
        }),
      priority: 10,
    })

    // ── Activity-bar item ─────────────────────────────────────────────
    // Priority 55 sits between the AI chat (50) and Processes (60) so
    // the timeline lives next to the surface that produces its
    // entries.
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: TIMELINE_ICON_PATH,
      title: 'AI Activity',
      viewId: VIEW_ID,
      priority: 55,
    })

    // ── Activity-bar routing — same idiom as nexus.processes ──────────
    api.events.on<{ viewId: string | null }>(
      EVENT_ACTIVITY_BAR_ACTIVE_CHANGED,
      ({ viewId }) => {
        if (viewId === VIEW_ID) {
          void hydrateFromKernel(api)
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
      await hydrateFromKernel(api)
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })
    api.commands.register(COMMAND_CLEAR, () => {
      void clearTimeline(api)
    })

    // ── Bus subscription ──────────────────────────────────────────────
    //
    // The kernel publishes `com.nexus.ai.activity_appended` after
    // every successful append. PluginRegistry tracks the disposer
    // returned from `api.kernel.on` and sweeps it on plugin unload —
    // we don't need to teardown manually.
    let kernelUnsub: (() => void) | null = null

    const subscribeBus = async () => {
      if (kernelUnsub) return
      try {
        kernelUnsub = await api.kernel.on<ActivityEntry>(
          TOPIC_ACTIVITY_APPENDED,
          (_topic, payload) => {
            if (payload && typeof payload === 'object' && 'id' in payload) {
              useActivityTimelineStore.getState().prepend(payload)
            }
          },
        )
      } catch (err) {
        console.warn(
          '[nexus.activityTimeline] failed to subscribe to activity bus:',
          err,
        )
      }
    }

    const unsubscribeBus = () => {
      if (!kernelUnsub) return
      try {
        kernelUnsub()
      } catch (err) {
        console.warn(
          '[nexus.activityTimeline] unsubscribe failed:',
          err,
        )
      }
      kernelUnsub = null
    }

    // Lifecycle: subscribe on workspace open, tear down on close.
    // Same pattern as nexus.processes — the kernel only exists
    // between `boot_kernel` and `shutdown`.
    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void hydrateFromKernel(api)
      void subscribeBus()
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      unsubscribeBus()
      // Leave the store populated — opening the next workspace
      // re-hydrates with that forge's log. Clearing here would
      // briefly flash an empty pane on tab-switch.
    })

    // Cover the restore-on-boot race: nexus.workspace may have
    // emitted `workspace:opened` before our listener attached. If
    // the kernel is up, hydrate + subscribe immediately.
    if (await api.kernel.available()) {
      void hydrateFromKernel(api)
      void subscribeBus()
    }
  },
}
