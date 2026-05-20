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
//
// Previously lived under shell/src/plugins/nexus/multibufferSync/ as
// a standalone plugin; folded into nexus.editor in Phase 4.6 since
// no other plugin consumed it.

import type { PluginAPI } from '../../../../types/plugin'
import type { EditorSnapshot } from '../types.ts'
import { EditorKernelClient } from '../kernelClient'
import { clientLogger } from '../../../../clientLogger'
import {
  CHANGED_TOPIC_PREFIX,
  changedTopicRelpath,
  extractSources,
  isMultibufferRelpath,
  multibuffersWatchingSource,
  type MultibufferRegistry,
} from './multibufferRegistry'

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

/**
 * Wire up multibuffer source-tracking and refresh-on-change.
 * Called once from `nexus.editor`'s `activate`. Idempotent only by
 * virtue of being called once; if reinvoked it would attach a second
 * subscriber chain.
 */
export function startMultibufferSync(api: PluginAPI): void {
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
          '[nexus.editor/multibufferSync] register failed:',
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
                '[nexus.editor/multibufferSync] refresh_excerpts failed:',
                watcher,
                err,
              )
            })
          }
        },
      )
    } catch (err) {
      clientLogger.warn('[nexus.editor/multibufferSync] subscribe failed:', err)
      unsub = null
    }
  }

  // Subscribe immediately — the editor plugin's IPC handlers are
  // always available once activate runs; no workspace gate needed
  // because the registry is empty until a multibuffer opens.
  void subscribe()
}
