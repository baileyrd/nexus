// BL-141 Approach B step 3b — multibuffer external-edit sync.
//
// Watches `files:open` events for `multibuffer://<uuid>` relpaths;
// when one opens, fetches the snapshot once and registers the unique
// source files its Excerpt blocks cover. A single global subscriber
// on `com.nexus.editor.changed.` (the topic the Rust editor publishes
// after every session mutation) walks the registry and calls
// `editor.refresh_excerpts(<multibuffer>)` for each multibuffer
// whose sources include the changed file. The refresh handler
// re-reads the source through storage IPC and replaces each
// Excerpt's content snapshot in place; block ids stay stable so
// cursor anchors survive.
//
// The refresh itself fires another `changed` event scoped to the
// multibuffer relpath. We filter those out at the subscriber to
// avoid an infinite loop.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import type { EditorSnapshot } from '../editor/types.ts'
import { EditorKernelClient } from '../editor/kernelClient'
import { clientLogger } from '../../../clientLogger'
import {
  CHANGED_TOPIC_PREFIX,
  changedTopicRelpath,
  extractSources,
  isMultibufferRelpath,
  multibuffersWatchingSource,
  type MultibufferRegistry,
} from './multibufferRegistry'

const PLUGIN_ID = 'nexus.multibufferSync'
const EVENT_FILES_OPEN = 'files:open'

/** Tagged separately from the test-facing pure helpers so the
 *  subscriber's wire-up stays narrow and replaceable. */
interface RegistryActions {
  /** Initial-snapshot fetch + register. Called when a `files:open`
   *  for a multibuffer relpath fires. Idempotent — re-registering
   *  the same relpath overwrites its source set (covers the case
   *  where a multibuffer's excerpts change after open). */
  register(relpath: string): Promise<void>
  /** Drop the registry entry — used when `refresh_excerpts` errors
   *  with session-not-found (the multibuffer's tab was closed). */
  unregister(relpath: string): void
}

export const multibufferSyncPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Multibuffer Sync',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.editor'],
  },

  async activate(api: PluginAPI) {
    const editorClient = new EditorKernelClient(api.kernel)
    const registry: MultibufferRegistry = new Map()

    const actions: RegistryActions = {
      async register(relpath) {
        try {
          const snap: EditorSnapshot = await editorClient.openSession(relpath)
          const sources = new Set(extractSources(snap))
          if (sources.size === 0) {
            // A multibuffer with no excerpts — nothing to watch.
            registry.delete(relpath)
            return
          }
          registry.set(relpath, { sources })
        } catch (err) {
          clientLogger.debug(
            '[nexus.multibufferSync] register failed:',
            relpath,
            err,
          )
        }
      },
      unregister(relpath) {
        registry.delete(relpath)
      },
    }

    api.events.on<{ relpath?: string }>(EVENT_FILES_OPEN, (payload) => {
      const relpath = payload?.relpath
      if (typeof relpath !== 'string') return
      if (!isMultibufferRelpath(relpath)) return
      void actions.register(relpath)
    })

    let unsub: (() => void) | null = null
    const subscribe = async () => {
      if (unsub) return
      try {
        unsub = await api.kernel.on<unknown>(
          CHANGED_TOPIC_PREFIX,
          (topic) => {
            const source = changedTopicRelpath(topic)
            if (!source) return
            // Ignore changed events scoped to a multibuffer's own
            // relpath — those are echoes of our own refresh.
            if (isMultibufferRelpath(source)) return
            const watchers = multibuffersWatchingSource(registry, source)
            for (const watcher of watchers) {
              editorClient.refreshExcerpts(watcher).catch((err) => {
                const msg = err instanceof Error ? err.message : String(err)
                // The synthetic session is gone — the tab closed.
                // Prune the registry entry so we stop trying.
                if (
                  msg.includes('not found') ||
                  msg.includes('not a multibuffer')
                ) {
                  actions.unregister(watcher)
                  return
                }
                clientLogger.debug(
                  '[nexus.multibufferSync] refresh_excerpts failed:',
                  watcher,
                  err,
                )
              })
            }
          },
        )
      } catch (err) {
        clientLogger.warn('[nexus.multibufferSync] subscribe failed:', err)
        unsub = null
      }
    }

    // Subscribe immediately — the editor plugin's IPC handlers are
    // always available once activate runs; no workspace gate needed
    // because the registry is empty until a multibuffer opens.
    void subscribe()
  },
}
