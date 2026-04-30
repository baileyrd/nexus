// Pure-logic tests for the BL-048 phase-3 drag-bridge factory.
// Re-exported via `shell/tests/block-ref-drag-bridge.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  createBlockRefDragBridge,
  type BlockRefDragBridgeDeps,
  type BlockRefSnapshot,
} from './blockRefDragBridge.ts'

const A_UUID = 'd8e9f0a1-2b3c-4d5e-9f01-abcdef012345'
const B_UUID = '11111111-2222-3333-4444-555555555555'

/** Build a snapshot whose root_blocks listing matches the supplied
 *  ids. Each block carries a synthetic `content` derived from its
 *  id so the label-truncation rule has something to read. */
function snapshot(ids: string[]): BlockRefSnapshot {
  const blocks: Record<string, { content?: string }> = {}
  for (const id of ids) blocks[id] = { content: `body for ${id}` }
  return { tree: { root_blocks: ids, blocks } }
}

interface StubClient {
  stampCalls: Array<{ relpath: string; blockId: string }>
  saveCalls: string[]
  stampBlock(
    relpath: string,
    blockId: string,
  ): Promise<{ block_id: string; stable_id: string; newly_stamped: boolean }>
  saveSession(relpath: string): Promise<void>
}

function stubClient(
  options: {
    stamp?: (
      relpath: string,
      blockId: string,
    ) => Promise<{ block_id: string; stable_id: string; newly_stamped: boolean }>
    save?: (relpath: string) => Promise<void>
  } = {},
): StubClient {
  const out: StubClient = {
    stampCalls: [],
    saveCalls: [],
    async stampBlock(relpath, blockId) {
      out.stampCalls.push({ relpath, blockId })
      if (options.stamp) return options.stamp(relpath, blockId)
      return {
        block_id: blockId,
        stable_id: A_UUID,
        newly_stamped: true,
      }
    },
    async saveSession(relpath) {
      out.saveCalls.push(relpath)
      if (options.save) return options.save(relpath)
    },
  }
  return out
}

interface Harness {
  deps: BlockRefDragBridgeDeps
  client: StubClient
  setActiveRelpath: (relpath: string | null) => void
  setSnapshot: (relpath: string, snap: BlockRefSnapshot | null) => void
  warnLog: Array<{ message: string; error: unknown }>
}

function makeHarness(initial: { relpath: string; ids: string[] }): Harness {
  let activeRelpath: string | null = initial.relpath
  const snapshots = new Map<string, BlockRefSnapshot | null>()
  snapshots.set(initial.relpath, snapshot(initial.ids))
  const warnLog: Harness['warnLog'] = []
  const client = stubClient()
  const deps: BlockRefDragBridgeDeps = {
    getActiveRelpath: () => activeRelpath,
    getSnapshot: (relpath) => snapshots.get(relpath) ?? null,
    client,
    warn: (message, error) => warnLog.push({ message, error }),
  }
  return {
    deps,
    client,
    setActiveRelpath: (relpath) => {
      activeRelpath = relpath
    },
    setSnapshot: (relpath, snap) => {
      snapshots.set(relpath, snap)
    },
    warnLog,
  }
}

// ── resolve passthrough ──────────────────────────────────────────────────────

test('resolve: returns null when no active relpath', () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  h.setActiveRelpath(null)
  const bridge = createBlockRefDragBridge(h.deps)
  assert.equal(bridge.resolve(0), null)
})

test('resolve: rejects untitled relpaths', () => {
  const h = makeHarness({ relpath: 'untitled-1', ids: ['det:0'] })
  const bridge = createBlockRefDragBridge(h.deps)
  assert.equal(bridge.resolve(0), null)
})

test('resolve: returns null for blockIndex out of range', () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  const bridge = createBlockRefDragBridge(h.deps)
  assert.equal(bridge.resolve(-1), null)
  assert.equal(bridge.resolve(7), null)
})

test('resolve: returns the snapshot id pre-stamp', () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0', 'det:1'] })
  const bridge = createBlockRefDragBridge(h.deps)
  assert.deepEqual(bridge.resolve(0), {
    relpath: 'A.md',
    blockId: 'det:0',
    label: 'body for det:0',
  })
})

test('resolve: truncates the label past 64 chars with ellipsis', () => {
  const longContent = 'x'.repeat(120)
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  h.setSnapshot('A.md', {
    tree: { root_blocks: ['det:0'], blocks: { 'det:0': { content: longContent } } },
  })
  const bridge = createBlockRefDragBridge(h.deps)
  const out = bridge.resolve(0)
  assert.ok(out)
  assert.ok(out.label && out.label.endsWith('…'))
  assert.equal(out.label.length, 64)
})

// ── stamp happy path ─────────────────────────────────────────────────────────

test('stamp: calls stampBlock + saveSession and caches the stable id', async () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  h.client.stampBlock = async (relpath, blockId) => {
    h.client.stampCalls.push({ relpath, blockId })
    return { block_id: blockId, stable_id: A_UUID, newly_stamped: true }
  }
  const bridge = createBlockRefDragBridge(h.deps)
  const stamped = await bridge.stamp!(0)
  assert.deepEqual(stamped, {
    relpath: 'A.md',
    blockId: A_UUID,
    label: 'body for det:0',
  })
  assert.deepEqual(h.client.stampCalls, [{ relpath: 'A.md', blockId: 'det:0' }])
  assert.deepEqual(h.client.saveCalls, ['A.md'])
})

test('resolve: returns the stamped id after a successful stamp (no snapshot refresh required)', async () => {
  // The dragstart path is sync — `resolve` must return the stable
  // UUID even if the kernel snapshot hasn't yet refreshed via the
  // changed-event subscription. The bridge's local cache covers
  // the gap.
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  const bridge = createBlockRefDragBridge(h.deps)
  await bridge.stamp!(0)
  // Snapshot still has the deterministic id — unchanged.
  assert.equal(h.deps.getSnapshot('A.md')?.tree.root_blocks[0], 'det:0')
  // Resolve returns the stamped id from the cache.
  assert.equal(bridge.resolve(0)?.blockId, A_UUID)
})

// ── stamp idempotence + dedup ────────────────────────────────────────────────

test('stamp: short-circuits when the snapshot already reports a UUID', async () => {
  const h = makeHarness({ relpath: 'A.md', ids: [B_UUID] })
  const bridge = createBlockRefDragBridge(h.deps)
  const out = await bridge.stamp!(0)
  assert.equal(out?.blockId, B_UUID)
  // No IPC fired because the block was already stable.
  assert.equal(h.client.stampCalls.length, 0)
  assert.equal(h.client.saveCalls.length, 0)
})

test('stamp: a second call after a successful stamp uses the cache and does not re-IPC', async () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  const bridge = createBlockRefDragBridge(h.deps)
  await bridge.stamp!(0)
  await bridge.stamp!(0)
  assert.equal(h.client.stampCalls.length, 1)
  assert.equal(h.client.saveCalls.length, 1)
})

test('stamp: concurrent calls dedupe to a single in-flight IPC', async () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  // Stall the stamp so both calls land while the first is in flight.
  let release: (
    value: { block_id: string; stable_id: string; newly_stamped: boolean },
  ) => void = () => undefined
  const stampPending = new Promise<{ block_id: string; stable_id: string; newly_stamped: boolean }>(
    (r) => {
      release = r
    },
  )
  h.client.stampBlock = (relpath, blockId) => {
    h.client.stampCalls.push({ relpath, blockId })
    return stampPending
  }
  const bridge = createBlockRefDragBridge(h.deps)
  const a = bridge.stamp!(0)
  const b = bridge.stamp!(0)
  release({ block_id: 'det:0', stable_id: A_UUID, newly_stamped: true })
  const [resA, resB] = await Promise.all([a, b])
  assert.equal(resA?.blockId, A_UUID)
  assert.equal(resB?.blockId, A_UUID)
  // Only one round-trip even though the user spam-hovered.
  assert.equal(h.client.stampCalls.length, 1)
})

// ── stamp failure paths ──────────────────────────────────────────────────────

test('stamp: a save failure does not reject the stamp; warn logged', async () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  h.client.saveSession = async () => {
    h.client.saveCalls.push('A.md')
    throw new Error('disk full')
  }
  const bridge = createBlockRefDragBridge(h.deps)
  const out = await bridge.stamp!(0)
  // Stamp itself succeeded — kernel has the stamped tree in
  // memory; user's next manual save will persist it.
  assert.equal(out?.blockId, A_UUID)
  assert.equal(h.warnLog.length, 1)
  assert.match(h.warnLog[0].message, /stamp save failed/)
})

test('stamp: a stampBlock IPC failure rejects and clears the in-flight slot for retry', async () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  let attempt = 0
  h.client.stampBlock = async (relpath, blockId) => {
    attempt += 1
    h.client.stampCalls.push({ relpath, blockId })
    if (attempt === 1) throw new Error('kernel hiccup')
    return { block_id: blockId, stable_id: A_UUID, newly_stamped: true }
  }
  const bridge = createBlockRefDragBridge(h.deps)
  await assert.rejects(bridge.stamp!(0), /kernel hiccup/)
  // Retry path — the in-flight slot was cleared so a second call
  // re-enters the IPC instead of returning the rejected promise.
  const out = await bridge.stamp!(0)
  assert.equal(out?.blockId, A_UUID)
  assert.equal(h.client.stampCalls.length, 2)
})

test('stamp: returns null when the snapshot is missing (closed session)', async () => {
  const h = makeHarness({ relpath: 'A.md', ids: ['det:0'] })
  h.setSnapshot('A.md', null)
  const bridge = createBlockRefDragBridge(h.deps)
  const out = await bridge.stamp!(0)
  assert.equal(out, null)
  assert.equal(h.client.stampCalls.length, 0)
})
