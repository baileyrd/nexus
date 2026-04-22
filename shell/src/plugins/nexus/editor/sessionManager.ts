// Refcounted session manager for the `com.nexus.editor` plugin.
//
// Multiple UI surfaces (the editor tab, outline, a second split of the
// same file) can share a single kernel-side `Session`. This manager
// tracks how many holders are currently interested in `relpath` and
// opens/closes the underlying session at the 0→1 and 1→0 transitions.
//
// Untitled tabs (empty / placeholder relpaths) have no backing file and
// must not round-trip through the Rust plugin — `acquire`/`release`
// return `null` for those. Callers check the result before threading a
// snapshot into anything that needs one.
//
// Phase 3 scope: session refcount + snapshot cache.
// Phase 4 scope: subscribe to `com.nexus.editor.changed.<relpath>`
// custom events on the kernel bus; feed them into `editorStore` for
// revision tracking and echo suppression; fan out to local observers
// via a `changed` event emitter.

import type { KernelAPI } from '../../../types/plugin.ts'
import type {
  EditorChangedPayload,
  EditorSnapshot,
  TransactionId,
} from './types.ts'
import type { EditorKernelClient } from './kernelClient.ts'
import { useEditorStore } from './editorStore.ts'

/**
 * A relpath that has no backing file on disk. Untitled placeholder
 * paths use `untitled-N` — the editor store treats these as local-only
 * buffers, so we never open a kernel session for them.
 */
function isUntitledRelpath(relpath: string): boolean {
  return /^untitled-\d+$/i.test(relpath)
}

/**
 * True iff `relpath` is eligible for a kernel session. Empty / null /
 * untitled relpaths return `false`; callers short-circuit to a
 * `null` acquire result.
 */
function isSessionableRelpath(relpath: string | null | undefined): relpath is string {
  if (!relpath) return false
  if (relpath.length === 0) return false
  if (isUntitledRelpath(relpath)) return false
  return true
}

/** Prefix used when subscribing to editor change events. Mirrors
 *  `EVENT_CHANGED_PREFIX` in `crates/nexus-editor/src/core_plugin.rs`. */
const CHANGED_EVENT_PREFIX = 'com.nexus.editor.changed.'

interface Entry {
  count: number
  /** Cached snapshot from the initial `openSession`; subsequent
   *  acquires return this verbatim without a fresh kernel call. */
  snapshot: EditorSnapshot
  /** `null` when no `KernelAPI` was supplied (unit tests) or when the
   *  subscription promise is still in flight. Populated on resolve; a
   *  `release` that races the subscription setup awaits it before
   *  unsubscribing so we don't leak a forwarder task. */
  unsubscribe: (() => void) | null
  /** Tracks the in-flight `api.kernel.on` promise so `release` can
   *  await it before tearing down. */
  subscribing: Promise<void> | null
}

/**
 * Listener for the local `changed` fan-out. Receives the full
 * [`EditorChangedPayload`] published by the Rust plugin *after* the
 * store has been updated and *after* echo suppression has run — i.e.
 * echoes of the shell's own in-flight transactions are NOT delivered.
 */
export type EditorChangedListener = (payload: EditorChangedPayload) => void

/**
 * Tracks per-relpath refcount + cached snapshot over an
 * {@link EditorKernelClient}. Not thread-safe by design — the editor
 * lifecycle is driven from the main thread and the underlying IPC is
 * already serialized on the kernel side.
 *
 * When constructed with a `KernelAPI`, the manager also owns one
 * kernel-event subscription per open session. Handlers route the
 * `com.nexus.editor.changed.<relpath>` payload through:
 *   1. `editorStore.consumePendingLocalRevision(transaction_id)` to
 *      drop echoes of the shell's own dispatches (always false in
 *      Phase 4 — the set is only populated by the Phase 5 transaction
 *      bridge — but wiring the check now means Phase 5 is a one-line
 *      change).
 *   2. `editorStore.setSessionRevision(relpath, revision)` for
 *      consumers that want to know "what's the latest revision I've
 *      seen for this file?".
 *   3. `emit('changed', payload)` on the manager's own listener list,
 *      so Phase 7 consumers (outline, backlinks, second editor tab)
 *      can react without poking at the store.
 *
 * The listener API intentionally avoids Zustand — the outline / graph
 * panes are not always mounted at subscribe time, and a plain
 * add/remove listener pair is enough for their needs.
 */
export class SessionManager {
  private readonly client: EditorKernelClient
  /** Optional — when `null`, `acquire` skips the subscription path and
   *  `changed` listeners never fire. Kept optional so the existing
   *  unit tests (which drive the manager without a full runtime) keep
   *  working as-is. */
  private readonly api: KernelAPI | null
  private readonly entries = new Map<string, Entry>()
  private readonly changedListeners = new Set<EditorChangedListener>()

  constructor(client: EditorKernelClient, api: KernelAPI | null = null) {
    this.client = client
    this.api = api
  }

  /**
   * Increment the refcount for `relpath`. On the 0→1 transition, opens
   * a kernel session and caches the returned snapshot. Subsequent
   * `acquire` calls for the same path return the cached snapshot
   * without a second kernel round-trip.
   *
   * Returns `null` for untitled / empty relpaths — those don't map to
   * kernel sessions, and callers should fall back to local-only state.
   */
  async acquire(relpath: string | null | undefined): Promise<EditorSnapshot | null> {
    if (!isSessionableRelpath(relpath)) return null
    const existing = this.entries.get(relpath)
    if (existing) {
      existing.count += 1
      return existing.snapshot
    }
    const snapshot = await this.client.openSession(relpath)
    // Seed the store's known revision from the open-time snapshot so
    // post-open consumers can read a consistent starting value.
    useEditorStore.getState().setSessionRevision(relpath, snapshot.revision)
    // Phase 6: a freshly-opened session mirrors what's on disk, so
    // `savedRevision` starts equal to `sessionRevision`. Any local
    // edit (bridge → setSessionRevision) then diverges the two and
    // `isDirty` flips to true.
    useEditorStore.getState().markSavedRevision(relpath)
    const entry: Entry = {
      count: 1,
      snapshot,
      unsubscribe: null,
      subscribing: null,
    }
    this.entries.set(relpath, entry)

    if (this.api) {
      // Subscribe to this specific file's changed channel. `on` is
      // prefix-based, so a full `type_id` also matches exactly — we
      // pass the fully-qualified relpath to minimise cross-file
      // wakeups on the Tauri event bridge.
      const topicPrefix = `${CHANGED_EVENT_PREFIX}${relpath}`
      const subscribing = this.api
        .on<EditorChangedPayload>(topicPrefix, (topic, payload) =>
          this.handleChanged(topic, payload),
        )
        .then((unsub) => {
          // Only install the unsubscribe handle if the entry is still
          // live — a race where release() ran before the subscription
          // resolved would otherwise leak the Rust-side forwarder.
          const current = this.entries.get(relpath)
          if (current === entry) {
            entry.unsubscribe = unsub
          } else {
            try {
              unsub()
            } catch {
              // unsubscribe is best-effort — never throw from here.
            }
          }
        })
        .catch(() => {
          // Subscription failures degrade to "no live updates" —
          // acquire still succeeds so the tab is usable.
        })
      entry.subscribing = subscribing
    }

    return snapshot
  }

  /**
   * Decrement the refcount for `relpath`. On the 1→0 transition, closes
   * the kernel session and drops the cached snapshot. No-op for
   * untitled relpaths or paths that were never acquired.
   */
  async release(relpath: string | null | undefined): Promise<void> {
    if (!isSessionableRelpath(relpath)) return
    const entry = this.entries.get(relpath)
    if (!entry) return
    entry.count -= 1
    if (entry.count <= 0) {
      this.entries.delete(relpath)
      // Wait for any in-flight subscription to resolve so we can tear
      // it down — racing a release against an unfinished `on` would
      // otherwise leave the forwarder task alive.
      if (entry.subscribing) {
        try {
          await entry.subscribing
        } catch {
          // already swallowed in the subscribe chain
        }
      }
      if (entry.unsubscribe) {
        try {
          entry.unsubscribe()
        } catch {
          // best-effort teardown
        }
      }
      useEditorStore.getState().clearSessionRevision(relpath)
      useEditorStore.getState().clearSavedRevision(relpath)
      await this.client.closeSession(relpath)
    }
  }

  /**
   * Current refcount for `relpath`. Test helper — production callers
   * should round-trip through `acquire` / `release`.
   */
  refcount(relpath: string): number {
    return this.entries.get(relpath)?.count ?? 0
  }

  /**
   * Return the cached snapshot for `relpath`, or `null` when the
   * session isn't open. Used by the Phase 5 transaction bridge to
   * resolve `tree.root_blocks[0]` at dispatch time. Note: this is the
   * *open-time* snapshot — subsequent edits advance the kernel-side
   * revision, but the tree shape on v1's coarse-block path doesn't
   * change (still a single root). Callers who care about freshness
   * should call `client.getTree(relpath)` directly.
   */
  getSnapshot(relpath: string): EditorSnapshot | null {
    return this.entries.get(relpath)?.snapshot ?? null
  }

  /**
   * Register a listener for `changed` events. Returns an unsubscribe
   * function. Listener receives the canonical
   * [`EditorChangedPayload`] (relpath, revision, transaction_id);
   * echoes of in-flight local transactions have already been filtered
   * out, so every delivery represents an externally-originated change
   * the consumer should reconcile with.
   *
   * Intentionally synchronous — Phase 7 consumers (outline /
   * backlinks) add a listener at mount and drop it at unmount without
   * needing a store selector.
   */
  onChanged(listener: EditorChangedListener): () => void {
    this.changedListeners.add(listener)
    return () => {
      this.changedListeners.delete(listener)
    }
  }

  /**
   * Internal handler invoked by the `api.kernel.on` forwarder. Filters
   * echoes via `pendingLocalRevisions` before touching the store.
   *
   * Exposed as a named method (instead of an inline closure) so tests
   * can drive it directly.
   */
  private handleChanged(_topic: string, payload: EditorChangedPayload): void {
    // Echo suppression: if this transaction id is in the pending set,
    // the local dispatcher has already reconciled the snapshot — drop
    // the event entirely so downstream consumers don't double-apply.
    const txId = payload.transaction_id
    if (txId !== null) {
      const consumed = useEditorStore
        .getState()
        .consumePendingLocalRevision(txId as TransactionId)
      if (consumed) return
    }
    useEditorStore
      .getState()
      .setSessionRevision(payload.relpath, payload.revision)
    for (const listener of this.changedListeners) {
      try {
        listener(payload)
      } catch {
        // A misbehaving listener must not prevent the rest from firing.
      }
    }
  }
}

/** Factory mirror of `makeEditorClient` — keeps test call sites uniform.
 *  Pass `api` so the manager can open a per-session subscription; omit
 *  it for unit tests that exercise refcount semantics without a live
 *  kernel. */
export function makeSessionManager(
  client: EditorKernelClient,
  api: KernelAPI | null = null,
): SessionManager {
  return new SessionManager(client, api)
}
