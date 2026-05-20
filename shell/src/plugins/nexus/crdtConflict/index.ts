// BL-007 / BL-074 — surface CRDT pull-landing conflicts to the user.
//
// `crates/nexus-bootstrap/src/crdt_publisher.rs::publish_conflicts`
// fires `com.nexus.editor.crdt.conflict.<relpath>` whenever a `git
// pull` lands an op the live session can't merge silently
// (`StructuralDeleteEdit` or whole-block-replacement
// `ConcurrentBlockEdit`). Without a subscriber the user gets no
// signal — the file appears to silently revert or skip remote edits
// because the conflicting op was buffered away from the doc.
//
// As of BL-074 the surface is an interactive **resolver modal** (not
// the original toast). The Rust side enriches each conflict with
// content snapshots — `local_content`, `remote_content`,
// `delete_origin` — so the modal can render side-by-side and offer
// "Keep local" / "Use remote" / "Open file" without an extra
// round-trip.

import { createElement } from 'react'

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { ConflictModal } from './ConflictModal'
import { useConflictStore } from './conflictStore'
import type { ConflictEnvelope } from './types'

const TOPIC_PREFIX = 'com.nexus.editor.crdt.conflict.'
const VIEW_ID = 'nexus.crdtConflict.modal'

export const crdtConflictPlugin: Plugin = {
  manifest: {
    id: 'nexus.crdtConflict',
    name: 'CRDT Conflict',
    version: '0.2.0',
    core: false,
    activationEvents: ['onStartup'],
    // Dead weight without nexus.collab driving the CRDT publisher —
    // makes the soft requirement explicit so this plugin sorts after
    // collab when both are enabled.
    dependsOn: ['nexus.collab'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    let unsub: (() => void) | null = null

    api.views.register(VIEW_ID, {
      slot: 'overlay',
      // The modal needs `api.kernel.invoke` (apply_transaction) and
      // `api.events.emit` (files:open) — both threaded through as a
      // prop. Wrap in a closure component so the registry-supplied
      // factory can render without arguments.
      component: () => createElement(ConflictModal, { api }),
      // Same priority bucket as `confirm` and `pick`: an overlay
      // serialises behind the conflict store's queue+current pattern,
      // so two conflict modals can't render simultaneously.
      priority: 90,
    })

    const subscribe = async () => {
      if (unsub) return
      try {
        unsub = await api.kernel.on<ConflictEnvelope>(TOPIC_PREFIX, (topic, payload) => {
          const relpath = topic.slice(TOPIC_PREFIX.length)
          if (!payload || !Array.isArray(payload.conflicts) || payload.conflicts.length === 0) {
            return
          }
          clientLogger.info(
            '[nexus.crdtConflict]',
            relpath,
            `${payload.conflicts.length} conflict(s)`,
          )
          useConflictStore.getState().enqueue(relpath, payload.conflicts)
        })
      } catch (err) {
        clientLogger.warn('[nexus.crdtConflict] subscribe failed:', err)
        unsub = null
      }
    }

    const unsubscribe = () => {
      if (!unsub) return
      try {
        unsub()
      } catch (err) {
        clientLogger.warn('[nexus.crdtConflict] unsubscribe failed:', err)
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
