// BL-129 follow-up — Dream Cycle inbox + toast subscriber.
//
// Toast surface (shipped earlier) — subscribes to
// `com.nexus.dream_cycle.proposals` and fires an info toast announcing
// how many new relation proposals the nightly cycle produced.
//
// Inbox surface — paneMode view rendering one row per LLM-proposed
// relation (`confidence ≤ 0.5`) with Approve / Skip buttons. Approve
// bumps confidence to 1.0 via `entity_get` + `entity_upsert`; Skip
// drops the row from the entity's relation list the same way. The
// enumeration handler is `com.nexus.storage::list_draft_relations`.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { clientLogger } from '../../../clientLogger'
import { DreamCycleInboxView } from './DreamCycleInboxView'
import {
  rowKey,
  useDreamCycleStore,
  type DraftRelationRow,
} from './dreamCycleStore'

const PLUGIN_ID = 'nexus.dreamCycle'
const VIEW_ID = 'nexus.dreamCycle.view'
const ACTIVITY_ITEM_ID = 'nexus.dreamCycle.activityItem'
const COMMAND_SHOW = 'nexus.dreamCycle.show'
const COMMAND_REFRESH = 'nexus.dreamCycle.refresh'

const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const TOPIC = 'com.nexus.dream_cycle.proposals'
const APPROVE_CONFIDENCE = 1.0

/** Crescent moon — stroke-only, matches the iconPath contract used by
 *  the other activity-bar items. Lucide `moon`. */
const MOON_ICON_PATH = 'M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z'

/** Wire shape published by `nexus-bootstrap`'s `dream_cycle::run_cycle`. */
export interface DreamCycleProposalsPayload {
  proposals_total: number
  entities_enriched: number
  merged: number
  review: number
}

/** Compose the toast string. Pure helper so the shape stays tested
 *  without a kernel mock. */
export function composeToast(payload: DreamCycleProposalsPayload): string {
  const total = Number.isFinite(payload.proposals_total) ? payload.proposals_total : 0
  if (total <= 0) return ''
  const noun = total === 1 ? 'proposal' : 'proposals'
  return `${total} new relation ${noun} from Dream Cycle`
}

/** Re-fetch the inbox from `list_draft_relations`. Idempotent on the
 *  store — overwrites the row list each call. */
async function hydrate(api: PluginAPI): Promise<void> {
  try {
    const reply = await api.kernel.invoke<{
      relations: DraftRelationRow[]
      total: number
      truncated: boolean
    }>(STORAGE_PLUGIN_ID, 'list_draft_relations', { threshold: 0.5, limit: 200 })
    const rows = Array.isArray(reply?.relations) ? reply.relations : []
    const total = Number.isFinite(reply?.total) ? reply.total : rows.length
    const truncated = Boolean(reply?.truncated)
    useDreamCycleStore.getState().hydrate(rows, total, truncated)
  } catch (err) {
    clientLogger.debug('[nexus.dreamCycle] list_draft_relations failed:', err)
    useDreamCycleStore.getState().hydrate([], 0, false)
  }
}

/** Shape of `com.nexus.storage::entity_get` reply we care about. The
 *  full record carries fields we don't touch (entity_type, aliases,
 *  description) — we round-trip them verbatim through entity_upsert. */
interface EntityRecord {
  id: string
  entity_type: string
  aliases: string[]
  description: string
  relations: Array<{ target: string; type: string; confidence: number }>
  relpath: string
}

/** Find the `(target, kind)` relation in an entity's list. Returns the
 *  index or `-1` when no match. Exported for tests. */
export function findRelationIndex(
  entity: EntityRecord,
  target: string,
  kind: string,
): number {
  return entity.relations.findIndex(
    (r) => r.target === target && r.type === kind,
  )
}

/** Build the entity_upsert payload from an existing record + a relation
 *  list transform. The transform mutates the list to either bump
 *  confidence (approve) or drop the row (skip). */
export function buildUpsertPayload(
  entity: EntityRecord,
  relations: EntityRecord['relations'],
): Record<string, unknown> {
  return {
    id:          entity.id,
    entity_type: entity.entity_type,
    aliases:     entity.aliases,
    description: entity.description,
    relations:   relations.map((r) => ({
      target:     r.target,
      type:       r.type,
      confidence: r.confidence,
    })),
  }
}

/** Approve a draft — bump confidence to `APPROVE_CONFIDENCE` and
 *  `entity_upsert`. Skip — drop the relation row. Both flows funnel
 *  through this helper so the optimistic-removal + pending-flag
 *  bookkeeping stays consistent. */
async function applyAction(
  api: PluginAPI,
  row: DraftRelationRow,
  action: 'approve' | 'skip',
): Promise<void> {
  const key = rowKey(row)
  const store = useDreamCycleStore.getState()
  store.markPending(key)
  try {
    const reply = await api.kernel.invoke<{ entity: EntityRecord | null }>(
      STORAGE_PLUGIN_ID,
      'entity_get',
      { id: row.from },
    )
    const entity = reply?.entity ?? null
    if (!entity) {
      clientLogger.warn(
        `[nexus.dreamCycle] ${action}: entity_get returned null for '${row.from}'`,
      )
      store.removeRow(key)
      return
    }
    const idx = findRelationIndex(entity, row.target, row.type)
    if (idx < 0) {
      // Relation already gone (concurrent edit, manual cleanup, …) —
      // mirror the action by removing the stale row.
      store.removeRow(key)
      return
    }
    const nextRelations = entity.relations.slice()
    if (action === 'approve') {
      nextRelations[idx] = { ...nextRelations[idx], confidence: APPROVE_CONFIDENCE }
    } else {
      nextRelations.splice(idx, 1)
    }
    await api.kernel.invoke(
      STORAGE_PLUGIN_ID,
      'entity_upsert',
      buildUpsertPayload(entity, nextRelations),
    )
    store.removeRow(key)
  } catch (err) {
    clientLogger.warn(`[nexus.dreamCycle] ${action} failed:`, err)
    // Re-hydrate from source of truth — the optimistic removal lost.
    void hydrate(api)
  } finally {
    useDreamCycleStore.getState().clearPending(key)
  }
}

export const dreamCyclePlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Dream Cycle',
    version: '0.2.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.paneMode', 'nexus.activityBar'],
    contributes: {
      commands: [
        {
          id: COMMAND_SHOW,
          title: 'Show Dream Cycle Inbox',
          category: 'Dream Cycle',
        },
        {
          id: COMMAND_REFRESH,
          title: 'Refresh Dream Cycle Inbox',
          category: 'Dream Cycle',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    // ── View registration ─────────────────────────────────────────────
    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(DreamCycleInboxView, {
          onApprove: (row) => {
            void applyAction(api, row, 'approve')
          },
          onSkip: (row) => {
            void applyAction(api, row, 'skip')
          },
          onRefresh: () => {
            void hydrate(api)
          },
        }),
      priority: 12,
    })

    // ── Activity-bar item ─────────────────────────────────────────────
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: MOON_ICON_PATH,
      title: 'Dream Cycle',
      viewId: VIEW_ID,
      priority: 58,
    })

    // ── Activity-bar routing ──────────────────────────────────────────
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

    // ── Bus subscription — toast + refresh on new proposals ───────────
    let unsub: (() => void) | null = null

    const subscribe = async () => {
      if (unsub) return
      try {
        unsub = await api.kernel.on<DreamCycleProposalsPayload>(
          TOPIC,
          (_topic, payload) => {
            if (!payload || typeof payload.proposals_total !== 'number') return
            const message = composeToast(payload)
            if (message) {
              api.notifications.show({ message, type: 'info' })
            }
            // Refresh the inbox so the new proposals show up the next
            // time the user opens the panel (or immediately if it's
            // already mounted).
            void hydrate(api)
          },
        )
      } catch (err) {
        clientLogger.warn('[nexus.dreamCycle] subscribe failed:', err)
        unsub = null
      }
    }

    const unsubscribe = () => {
      if (!unsub) return
      try {
        unsub()
      } catch (err) {
        clientLogger.warn('[nexus.dreamCycle] unsubscribe failed:', err)
      }
      unsub = null
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void hydrate(api)
      void subscribe()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      unsubscribe()
    })

    // Cover the boot race: workspace:opened may have fired before our
    // listener attached.
    if (await api.kernel.available()) {
      void hydrate(api)
      void subscribe()
    }
  },
}
