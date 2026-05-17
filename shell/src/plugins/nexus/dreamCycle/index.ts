// BL-129 follow-up — subscribe to `com.nexus.dream_cycle.proposals`
// and surface a toast announcing how many new relation proposals the
// nightly Dream Cycle produced. The kernel side (`nexus-bootstrap`
// `dream_cycle.rs`) publishes this event whenever
// `infer_entity_relations` writes draft relations at `confidence: 0.5`.
//
// Scope: this iteration ships the toast surface. The per-row
// approve / skip inbox in the BL DoD requires enumeration of draft
// relations, which the storage IPC does not yet expose — that piece
// is gated on a future `list_draft_relations` handler.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'

const TOPIC = 'com.nexus.dream_cycle.proposals'

/** Wire shape published by `nexus-bootstrap`'s `dream_cycle::run_cycle` */
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

export const dreamCyclePlugin: Plugin = {
  manifest: {
    id: 'nexus.dreamCycle',
    name: 'Dream Cycle',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    let unsub: (() => void) | null = null

    const subscribe = async () => {
      if (unsub) return
      try {
        unsub = await api.kernel.on<DreamCycleProposalsPayload>(
          TOPIC,
          (_topic, payload) => {
            if (!payload || typeof payload.proposals_total !== 'number') return
            const message = composeToast(payload)
            if (!message) return
            api.notifications.show({ message, type: 'info' })
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

    api.events.on('workspace:opened', () => {
      void subscribe()
    })
    api.events.on('workspace:closed', () => {
      unsubscribe()
    })

    if (await api.kernel.available()) {
      void subscribe()
    }
  },
}
