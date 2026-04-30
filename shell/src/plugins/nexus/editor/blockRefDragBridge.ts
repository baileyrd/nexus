// shell/src/plugins/nexus/editor/blockRefDragBridge.ts
//
// BL-048 phase 3 — factory for the `BlockRefDragBridge` impl that
// the editor plugin installs into the in-CM block handle. Lives in
// its own file so the cache + in-flight dedup + stamp dispatch can
// be unit-tested without standing up the whole `nexus.editor`
// activate() path.
//
// Owners:
//   * `nexus.editor.activate()` calls `createBlockRefDragBridge(deps)`
//     once and hands the result to `setBlockRefDragBridge(...)`.
//   * Tests build a fake deps object and assert the bridge's
//     resolve/stamp behaviour against it.
//
// See `cm/blockHandle.ts` (the consumer side — pre-stamp on hover,
// "Stamp block" right-click affordance) and
// `kernelClient.ts#stampBlock` (the IPC the bridge calls into).

import type { BlockRefDragBridge } from './cm/blockHandle'

/** A snapshot the bridge can read for `(relpath, blockId, label)`. */
export interface BlockRefSnapshot {
  tree: {
    root_blocks: string[]
    blocks: Record<string, { content?: string }>
  }
}

/** Subset of `EditorKernelClient` the bridge needs for stamping. */
export interface BridgeKernelClient {
  stampBlock(relpath: string, blockId: string): Promise<{
    block_id: string
    stable_id: string
    newly_stamped: boolean
  }>
  saveSession(relpath: string): Promise<void>
}

/** Dependencies the bridge factory needs. Keep narrow so tests
 *  can stub each piece without touching the wider editor plugin. */
export interface BlockRefDragBridgeDeps {
  /** Returns the active tab's relpath, or null when no tab is
   *  active. Untitled relpaths (`untitled-N`) are filtered upstream
   *  but the bridge also rejects them as a defence in depth. */
  getActiveRelpath: () => string | null | undefined
  /** Read the live snapshot for `relpath`, or null when the session
   *  isn't open. */
  getSnapshot: (relpath: string) => BlockRefSnapshot | null
  /** Editor IPC handles for stamping + saving. */
  client: BridgeKernelClient
  /** Optional logger for save failures. Defaults to a no-op so the
   *  console isn't noisy in tests. Hook this to `console.warn` in
   *  production. */
  warn?: (message: string, error: unknown) => void
}

const UNTITLED_RE = /^untitled-\d+$/i
const UUID_RE =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/

const LABEL_BUDGET = 64

interface ResolvedRef {
  relpath: string
  blockId: string
  label: string | null
}

function readBaseResolution(
  deps: BlockRefDragBridgeDeps,
  blockIndex: number,
): ResolvedRef | null {
  const relpath = deps.getActiveRelpath()
  if (!relpath) return null
  if (UNTITLED_RE.test(relpath)) return null
  const snapshot = deps.getSnapshot(relpath)
  if (!snapshot) return null
  const rootIds = snapshot.tree.root_blocks
  if (blockIndex < 0 || blockIndex >= rootIds.length) return null
  const blockId = rootIds[blockIndex]
  const block = snapshot.tree.blocks[blockId]
  const raw = (block?.content ?? '').replace(/\s+/g, ' ').trim()
  // Truncate at LABEL_BUDGET characters total, including the
  // trailing ellipsis. The ellipsis is one Unicode character so the
  // slice is `LABEL_BUDGET - 1`. Matches the cap the original
  // inline implementation in `editor/index.ts` documented (the old
  // `slice(0, 61)` + '…' produced 62 chars; phase 3 fixes that to
  // actually hit the documented 64 budget).
  const label =
    raw.length > LABEL_BUDGET ? `${raw.slice(0, LABEL_BUDGET - 1)}…` : raw || null
  return { relpath, blockId, label }
}

/** Build a `BlockRefDragBridge` from the supplied dependencies.
 *  The bridge keeps a private `(relpath, deterministic_id) →
 *  stable_id` cache so `resolve` can return the stamped id
 *  synchronously after a successful stamp; it also de-dupes
 *  in-flight stamp calls so a hover-spam + right-click sequence
 *  doesn't queue two IPCs for the same block. */
export function createBlockRefDragBridge(
  deps: BlockRefDragBridgeDeps,
): BlockRefDragBridge {
  /** key: `${relpath}::${pre_stamp_block_id}` → stable_id. */
  const stampedIds = new Map<string, string>()
  /** key: same; value: in-flight promise. Cleared on settle so a
   *  later retry after a transient kernel failure isn't blocked. */
  const inFlight = new Map<string, Promise<ResolvedRef | null>>()

  const cacheKey = (relpath: string, blockId: string) =>
    `${relpath}::${blockId}`

  return {
    resolve: (blockIndex) => {
      const base = readBaseResolution(deps, blockIndex)
      if (!base) return null
      const cached = stampedIds.get(cacheKey(base.relpath, base.blockId))
      if (cached) return { ...base, blockId: cached }
      return base
    },
    stamp: async (blockIndex) => {
      const base = readBaseResolution(deps, blockIndex)
      if (!base) return null
      const key = cacheKey(base.relpath, base.blockId)
      // Already stamped (UUID id reported by snapshot) — record the
      // identity so a follow-up `resolve` short-circuits and return
      // the resolution as-is.
      if (UUID_RE.test(base.blockId)) {
        stampedIds.set(key, base.blockId)
        return base
      }
      const cached = stampedIds.get(key)
      if (cached) return { ...base, blockId: cached }
      const inflight = inFlight.get(key)
      if (inflight) return inflight
      const run = (async () => {
        try {
          const stamp = await deps.client.stampBlock(base.relpath, base.blockId)
          // Persist so the `<!-- ^<uuid> -->` anchor lives on disk;
          // without the save a fresh session re-parses without the
          // marker and the stable id is lost on next reopen.
          try {
            await deps.client.saveSession(base.relpath)
          } catch (saveErr) {
            // Non-fatal for the drag — the kernel still has the
            // stamped tree in memory, and the user's next manual
            // save will persist it. Log so QA notices.
            deps.warn?.('[BL-048] stamp save failed', saveErr)
          }
          stampedIds.set(key, stamp.stable_id)
          return { ...base, blockId: stamp.stable_id }
        } finally {
          inFlight.delete(key)
        }
      })()
      inFlight.set(key, run)
      return run
    },
  }
}
