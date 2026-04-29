// Unit tests for workspace persistence. Uses node:test so no extra devDep
// is needed. Run with:
//   node --experimental-strip-types --test src/workspace/persistence.test.ts
//
// Same dynamic-import trick as the other workspace tests — keeps tsc
// happy without @types/node.

import type { WorkspaceJSON } from './types.ts'
import {
  __setKernelBridge,
  createDebouncedSaver,
  installAutoSave,
  loadWorkspace,
  saveWorkspace,
  type KernelBridge,
} from './persistence.ts'
import { buildDefaultLayout } from './defaultLayout.ts'
import { workspace } from './workspaceStore.ts'
import { viewRegistry } from './ViewRegistry.ts'
import { ViewBase } from './View.ts'
import type { Leaf, View } from './types.ts'

const nodeTest: string = 'node:test'
const nodeAssert: string = 'node:assert/strict'
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const { test } = (await import(nodeTest)) as any
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const assert = ((await import(nodeAssert)) as any).default

// ---------------------------------------------------------------------------
// Mock bridge — an in-memory map swappable per test. Also allows returning
// raw text to simulate malformed files.
// ---------------------------------------------------------------------------

interface MockBridge extends KernelBridge {
  setFile(relPath: string, text: string | null): void
  writes: Array<{ path: string; content: string }>
}

function makeMockBridge(): MockBridge {
  const files = new Map<string, string>()
  const writes: Array<{ path: string; content: string }> = []
  return {
    async readVaultFile(relPath: string): Promise<string | null> {
      return files.has(relPath) ? files.get(relPath)! : null
    },
    async writeVaultFile(relPath: string, content: string): Promise<void> {
      files.set(relPath, content)
      writes.push({ path: relPath, content })
    },
    setFile(relPath: string, text: string | null): void {
      if (text === null) files.delete(relPath)
      else files.set(relPath, text)
    },
    writes,
  }
}

// Register a couple of view types so hydrate can drive leaves. `empty` is
// already registered at module load by ViewRegistry. file-explorer / search /
// outline / backlink are what buildDefaultLayout references.
class DummyView extends ViewBase {
  readonly viewType: string
  constructor(leaf: Leaf, viewType: string) {
    super(leaf)
    this.viewType = viewType
  }
}
const ensureRegistered = (type: string): void => {
  if (!viewRegistry.getCreator(type)) {
    viewRegistry.register(type, (leaf): View => new DummyView(leaf, type))
  }
}
for (const t of ['file-explorer', 'search', 'outline', 'backlink']) {
  ensureRegistered(t)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test('loadWorkspace returns null when no file exists', async () => {
  const mock = makeMockBridge()
  const restore = __setKernelBridge(mock)
  try {
    const result = await loadWorkspace('/fake/vault')
    assert.equal(result, null)
  } finally {
    restore()
  }
})

test('loadWorkspace returns null for malformed JSON', async () => {
  const mock = makeMockBridge()
  mock.setFile('.forge/workspace.json', '{ this is not valid json')
  const restore = __setKernelBridge(mock)
  try {
    const result = await loadWorkspace('/fake/vault')
    assert.equal(result, null)
  } finally {
    restore()
  }
})

test('loadWorkspace returns null for schema-invalid JSON', async () => {
  const mock = makeMockBridge()
  // valid JSON, but missing required `main` / wrong kind.
  mock.setFile('.forge/workspace.json', JSON.stringify({ main: 'not-a-node' }))
  const restore = __setKernelBridge(mock)
  try {
    const result = await loadWorkspace('/fake/vault')
    assert.equal(result, null)
  } finally {
    restore()
  }
})

test('loadWorkspace returns parsed JSON for a valid layout', async () => {
  const mock = makeMockBridge()
  const layout = buildDefaultLayout()
  mock.setFile('.forge/workspace.json', JSON.stringify(layout))
  const restore = __setKernelBridge(mock)
  try {
    const result = await loadWorkspace('/fake/vault')
    assert.ok(result)
    assert.equal(result!.main.kind, 'split')
    assert.equal(result!.left.kind, 'split')
    assert.equal(result!.right.kind, 'split')
  } finally {
    restore()
  }
})

test('saveWorkspace writes pretty-printed JSON via the bridge', async () => {
  const mock = makeMockBridge()
  const restore = __setKernelBridge(mock)
  try {
    const layout = buildDefaultLayout()
    await saveWorkspace('/fake/vault', layout)
    assert.equal(mock.writes.length, 1)
    assert.equal(mock.writes[0]!.path, '.forge/workspace.json')
    // Parse round-trip must preserve structure.
    const reparsed = JSON.parse(mock.writes[0]!.content) as WorkspaceJSON
    assert.equal(reparsed.main.kind, 'split')
  } finally {
    restore()
  }
})

test('round-trip: serialize -> save -> load -> hydrate produces same tree shape', async () => {
  const mock = makeMockBridge()
  const restore = __setKernelBridge(mock)
  try {
    // Reset to a known state and seed.
    await workspace.hydrate(buildDefaultLayout())
    const serializedA = workspace.serialize()

    await saveWorkspace('/fake/vault', serializedA)
    const loaded = await loadWorkspace('/fake/vault')
    assert.ok(loaded)
    await workspace.hydrate(loaded!)
    const serializedB = workspace.serialize()

    // Compare tree kinds and leaf view types — ids and activeIndex round-trip.
    const shape = (j: WorkspaceJSON): unknown => ({
      mainKind: j.main.kind,
      leftSide: (j.left as { side?: string }).side,
      rightSide: (j.right as { side?: string }).side,
      // extract leaf types
      leafTypes: collectLeafTypes(j),
    })
    assert.deepEqual(shape(serializedA), shape(serializedB))
  } finally {
    restore()
  }
})

function collectLeafTypes(j: WorkspaceJSON): string[] {
  const out: string[] = []
  const walk = (node: unknown): void => {
    if (!node || typeof node !== 'object') return
    const n = node as { kind?: string; children?: unknown[]; leaves?: unknown[]; child?: unknown; viewState?: { type?: string } }
    if (n.kind === 'leaf' && n.viewState?.type) out.push(n.viewState.type)
    if (Array.isArray(n.children)) n.children.forEach(walk)
    if (Array.isArray(n.leaves)) n.leaves.forEach(walk)
    if (n.child) walk(n.child)
  }
  walk(j.main)
  walk(j.left)
  walk(j.right)
  return out
}

test('createDebouncedSaver coalesces rapid calls into one save', async () => {
  const mock = makeMockBridge()
  const restore = __setKernelBridge(mock)
  try {
    const save = createDebouncedSaver('/fake/vault', 30)
    const a = buildDefaultLayout()
    const b = buildDefaultLayout()
    const c = buildDefaultLayout()
    save(a)
    save(b)
    save(c)
    // Immediately after: no writes yet.
    assert.equal(mock.writes.length, 0)
    // Wait past the debounce window.
    await new Promise((r) => setTimeout(r, 80))
    assert.equal(mock.writes.length, 1)
    // Content matches the LAST call, not the first.
    const written = JSON.parse(mock.writes[0]!.content) as WorkspaceJSON
    assert.equal(written.active, c.active)
  } finally {
    restore()
  }
})

// --- BL-029: floating[] schema validation ----------------------------------

test('loadWorkspace accepts a workspace.json with floating[]', async () => {
  const mock = makeMockBridge()
  const layout = buildDefaultLayout()
  const withFloating: WorkspaceJSON = {
    ...layout,
    floating: [
      {
        kind: 'floating',
        id: 'fw-test',
        bounds: { x: 0, y: 0, w: 800, h: 600 },
        child: {
          kind: 'tabs',
          id: 'fw-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'fw-leaf', viewState: { type: 'empty' } },
          ],
        },
      },
    ],
  }
  mock.setFile('.forge/workspace.json', JSON.stringify(withFloating))
  const restore = __setKernelBridge(mock)
  try {
    const result = await loadWorkspace('/fake/vault')
    assert.ok(result)
    assert.ok(result!.floating)
    assert.equal(result!.floating!.length, 1)
    assert.equal(result!.floating![0]!.id, 'fw-test')
  } finally {
    restore()
  }
})

test('loadWorkspace rejects floating[] entries that are not floating nodes', async () => {
  const mock = makeMockBridge()
  const layout = buildDefaultLayout()
  // A tabs node where a floating node belongs is invalid for our schema.
  const bad = {
    ...layout,
    floating: [
      { kind: 'tabs', id: 't', activeIndex: 0, leaves: [] },
    ],
  }
  mock.setFile('.forge/workspace.json', JSON.stringify(bad))
  const restore = __setKernelBridge(mock)
  try {
    const result = await loadWorkspace('/fake/vault')
    assert.equal(result, null, 'floating[] with non-floating entry must be rejected')
  } finally {
    restore()
  }
})

test('loadWorkspace rejects non-array floating field', async () => {
  const mock = makeMockBridge()
  const layout = buildDefaultLayout()
  const bad = { ...layout, floating: { not: 'an array' } }
  mock.setFile('.forge/workspace.json', JSON.stringify(bad))
  const restore = __setKernelBridge(mock)
  try {
    const result = await loadWorkspace('/fake/vault')
    assert.equal(result, null)
  } finally {
    restore()
  }
})

test('installAutoSave triggers on layout-change and debounces', async () => {
  const mock = makeMockBridge()
  const restore = __setKernelBridge(mock)
  try {
    await workspace.hydrate(buildDefaultLayout())
    mock.writes.length = 0
    const stop = installAutoSave('/fake/vault')
    workspace.emit('layout-change')
    workspace.emit('layout-change')
    workspace.emit('view-changed')
    assert.equal(mock.writes.length, 0, 'no immediate writes')
    await new Promise((r) => setTimeout(r, 300))
    assert.ok(mock.writes.length >= 1, 'at least one debounced write fires')
    stop()
    const before = mock.writes.length
    workspace.emit('layout-change')
    await new Promise((r) => setTimeout(r, 300))
    assert.equal(mock.writes.length, before, 'no writes after disposer runs')
  } finally {
    restore()
  }
})
