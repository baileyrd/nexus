// BL-007 / BL-074 follow-up — surface CRDT pull-landing conflicts to
// the user.
//
// `crates/nexus-bootstrap/src/crdt_publisher.rs::publish_conflicts`
// fires `com.nexus.editor.crdt.conflict.<relpath>` whenever a `git
// pull` lands an op the live session can't merge silently
// (`StructuralDeleteEdit` or whole-block-replacement
// `ConcurrentBlockEdit`). Without a subscriber the user gets no
// signal — the file appears to silently revert or skip remote edits
// because the conflicting op was buffered away from the doc.
//
// This plugin is the lightest viable consumer: it subscribes to the
// topic prefix and surfaces a warning toast naming the file and the
// conflict shapes. A full resolver modal — pick local / pick remote
// per block — is a richer UX project tracked under BL-074 follow-ups
// and lives outside this plugin until then.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'

const TOPIC_PREFIX = 'com.nexus.editor.crdt.conflict.'

interface ConflictPayload {
  conflicts: Array<
    | { kind: 'concurrent_block_edit'; block_id: string; local: unknown; remote: unknown }
    | { kind: 'structural_delete_edit'; block_id: string; delete: unknown; edit: unknown }
  >
}

function summarise(payload: ConflictPayload): string {
  const counts = { concurrent_block_edit: 0, structural_delete_edit: 0 }
  for (const c of payload.conflicts) {
    if (c.kind === 'concurrent_block_edit') counts.concurrent_block_edit += 1
    else if (c.kind === 'structural_delete_edit') counts.structural_delete_edit += 1
  }
  const parts: string[] = []
  if (counts.concurrent_block_edit > 0) {
    parts.push(
      `${counts.concurrent_block_edit} concurrent block edit${counts.concurrent_block_edit === 1 ? '' : 's'}`,
    )
  }
  if (counts.structural_delete_edit > 0) {
    parts.push(
      `${counts.structural_delete_edit} delete-vs-edit conflict${counts.structural_delete_edit === 1 ? '' : 's'}`,
    )
  }
  return parts.join(', ')
}

export const crdtConflictPlugin: Plugin = {
  manifest: {
    id: 'nexus.crdtConflict',
    name: 'CRDT Conflict',
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
        unsub = await api.kernel.on<ConflictPayload>(TOPIC_PREFIX, (topic, payload) => {
          const relpath = topic.slice(TOPIC_PREFIX.length)
          if (!payload || !Array.isArray(payload.conflicts) || payload.conflicts.length === 0) {
            return
          }
          const summary = summarise(payload)
          clientLogger.warn('[nexus.crdtConflict]', relpath, payload.conflicts)
          api.notifications.show({
            type: 'warning',
            // Toast is intentionally action-less for now — a resolver
            // modal lands as a separate follow-up. Until then the
            // user opens the file and resolves manually.
            message: `Merge needs review in ${relpath}: ${summary}.`,
            duration: 8000,
          })
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
