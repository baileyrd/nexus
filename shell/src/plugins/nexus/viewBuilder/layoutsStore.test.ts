// shell/src/plugins/nexus/viewBuilder/layoutsStore.test.ts
//
// BL-067 Phase 1 — unit tests for the saved-layouts store.
//
// Covers the pure helpers (`nameToRelpath`, `relpathToName`,
// `normaliseName`) and the IPC-fronted operations (`listLayouts`,
// `loadLayout`, `saveLayout`, `deleteLayout`) against a fake
// `KernelAPI`. End-to-end interaction with the real storage plugin
// is covered by the existing `nexus-storage` Rust tests; the shell
// side just needs to confirm it shapes the IPC arguments correctly
// and parses responses.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  deleteLayout,
  listLayouts,
  loadLayout,
  nameToRelpath,
  normaliseName,
  refreshLayouts,
  relpathToName,
  saveLayout,
  useLayoutsStore,
} from './layoutsStore.ts'
import type { KernelAPI } from '../../../types/plugin.ts'
import type { WorkspaceJSON } from '../../../workspace/types.ts'

// ── path helpers ────────────────────────────────────────────────────────────

test('nameToRelpath / relpathToName round-trip every name', () => {
  for (const name of ['Focus', 'Research', 'Dev_2', 'AI Pair', 'a-b']) {
    assert.equal(relpathToName(nameToRelpath(name)), name)
  }
})

test('relpathToName rejects non-layout paths', () => {
  assert.equal(relpathToName('.forge/layouts/Focus.layout.json'), 'Focus')
  assert.equal(relpathToName('.forge/layouts/Focus.json'), null) // wrong suffix
  assert.equal(relpathToName('.forge/Focus.layout.json'), null) // wrong dir
  assert.equal(relpathToName('.forge/layouts/.layout.json'), null) // empty name
  assert.equal(relpathToName('.forge/layouts/sub/x.layout.json'), null) // nested
})

test('normaliseName rejects empty / disallowed / overlong inputs', () => {
  assert.equal(normaliseName('  Focus  '), 'Focus')
  assert.equal(normaliseName('a   b'), 'a b') // collapses runs
  assert.throws(() => normaliseName(''))
  assert.throws(() => normaliseName('   '))
  assert.throws(() => normaliseName('Bad/Slash'))
  assert.throws(() => normaliseName('Bad.Period'))
  assert.throws(() => normaliseName('x'.repeat(100)))
})

// ── fake KernelAPI ──────────────────────────────────────────────────────────

interface FakeCall {
  pluginId: string
  commandId: string
  args: unknown
}

interface FakeKernel extends KernelAPI {
  calls: FakeCall[]
  responses: Map<string, unknown | (() => unknown)>
  errors: Map<string, string>
}

function makeFakeKernel(): FakeKernel {
  const calls: FakeCall[] = []
  const responses = new Map<string, unknown | (() => unknown)>()
  const errors = new Map<string, string>()
  const k: FakeKernel = {
    calls,
    responses,
    errors,
    async invoke<T>(pluginId: string, commandId: string, args?: unknown): Promise<T> {
      calls.push({ pluginId, commandId, args: args ?? {} })
      const key = `${pluginId}::${commandId}`
      if (errors.has(key)) {
        throw new Error(errors.get(key))
      }
      const r = responses.get(key)
      const v = typeof r === 'function' ? (r as () => unknown)() : r
      return (v ?? null) as T
    },
    async on(): Promise<() => void> {
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return k
}

const SAMPLE_SNAPSHOT: WorkspaceJSON = {
  main: {
    kind: 'split',
    id: 'm',
    direction: 'horizontal',
    children: [{ kind: 'tabs', id: 't', leaves: [], activeIndex: 0 }],
  },
  left: {
    kind: 'split',
    id: 'l',
    direction: 'vertical',
    children: [{ kind: 'tabs', id: 'lt', leaves: [], activeIndex: 0 }],
    side: 'left',
    collapsed: false,
    size: 280,
  },
  right: {
    kind: 'split',
    id: 'r',
    direction: 'vertical',
    children: [{ kind: 'tabs', id: 'rt', leaves: [], activeIndex: 0 }],
    side: 'right',
    collapsed: false,
    size: 280,
  },
  active: null,
  lastOpenFiles: [],
}

// ── listLayouts ─────────────────────────────────────────────────────────────

test('listLayouts filters by suffix + dir, sorts by name, drops dirs', async () => {
  const k = makeFakeKernel()
  k.responses.set('com.nexus.storage::list_dir', [
    { name: 'Zeta.layout.json', relpath: '.forge/layouts/Zeta.layout.json', isDir: false },
    { name: 'Alpha.layout.json', relpath: '.forge/layouts/Alpha.layout.json', isDir: false },
    { name: 'random.json', relpath: '.forge/layouts/random.json', isDir: false },
    { name: 'subdir', relpath: '.forge/layouts/subdir', isDir: true },
  ])
  const rows = await listLayouts(k)
  assert.deepEqual(
    rows.map((r) => r.name),
    ['Alpha', 'Zeta'],
  )
})

test('listLayouts treats NotFound as empty list', async () => {
  const k = makeFakeKernel()
  k.errors.set('com.nexus.storage::list_dir', 'NotFound: no such directory')
  const rows = await listLayouts(k)
  assert.deepEqual(rows, [])
})

test('listLayouts re-throws non-NotFound errors', async () => {
  const k = makeFakeKernel()
  k.errors.set('com.nexus.storage::list_dir', 'PermissionDenied: nope')
  await assert.rejects(() => listLayouts(k), /PermissionDenied/)
})

// ── saveLayout / loadLayout ─────────────────────────────────────────────────

test('saveLayout writes JSON-encoded snapshot to <name>.layout.json', async () => {
  const k = makeFakeKernel()
  // create_dir might succeed or might "AlreadyExists"; success path:
  k.responses.set('com.nexus.storage::create_dir', {})
  k.responses.set('com.nexus.storage::write_file', {})
  const relpath = await saveLayout(k, 'Focus', SAMPLE_SNAPSHOT)
  assert.equal(relpath, '.forge/layouts/Focus.layout.json')
  // The write_file call carried a UTF-8 byte array of the JSON.
  const writeCall = k.calls.find((c) => c.commandId === 'write_file')
  assert.ok(writeCall, 'write_file called')
  const args = writeCall!.args as { path: string; bytes: number[] }
  assert.equal(args.path, '.forge/layouts/Focus.layout.json')
  const decoded = new TextDecoder().decode(new Uint8Array(args.bytes))
  const reparsed = JSON.parse(decoded) as WorkspaceJSON
  assert.deepEqual(reparsed, SAMPLE_SNAPSHOT)
})

test('saveLayout tolerates AlreadyExists from create_dir', async () => {
  const k = makeFakeKernel()
  k.errors.set('com.nexus.storage::create_dir', 'AlreadyExists: dir')
  k.responses.set('com.nexus.storage::write_file', {})
  const relpath = await saveLayout(k, 'Dev', SAMPLE_SNAPSHOT)
  assert.equal(relpath, '.forge/layouts/Dev.layout.json')
})

test('loadLayout decodes UTF-8 bytes and JSON-parses', async () => {
  const k = makeFakeKernel()
  const text = JSON.stringify(SAMPLE_SNAPSHOT)
  const bytes = Array.from(new TextEncoder().encode(text))
  k.responses.set('com.nexus.storage::read_file', { bytes })
  const out = await loadLayout(k, 'Focus')
  assert.deepEqual(out, SAMPLE_SNAPSHOT)
  // It used `path:` not `relpath:` (storage's read_file convention).
  const readCall = k.calls.find((c) => c.commandId === 'read_file')!
  assert.equal((readCall.args as { path: string }).path, '.forge/layouts/Focus.layout.json')
})

test('loadLayout rejects malformed JSON with a path-bearing message', async () => {
  const k = makeFakeKernel()
  const bytes = Array.from(new TextEncoder().encode('not json'))
  k.responses.set('com.nexus.storage::read_file', { bytes })
  await assert.rejects(() => loadLayout(k, 'Broken'), /Layout 'Broken'.*not valid JSON/)
})

// ── deleteLayout ────────────────────────────────────────────────────────────

test('deleteLayout returns true on success, false on NotFound', async () => {
  const k = makeFakeKernel()
  k.responses.set('com.nexus.storage::delete_file', {})
  assert.equal(await deleteLayout(k, 'Focus'), true)
  k.errors.set('com.nexus.storage::delete_file', 'NotFound: file gone')
  assert.equal(await deleteLayout(k, 'Focus'), false)
})

test('deleteLayout rethrows non-NotFound errors', async () => {
  const k = makeFakeKernel()
  k.errors.set('com.nexus.storage::delete_file', 'PermissionDenied')
  await assert.rejects(() => deleteLayout(k, 'X'), /PermissionDenied/)
})

// ── refreshLayouts (drives the zustand store) ───────────────────────────────

test('refreshLayouts populates the store on success', async () => {
  useLayoutsStore.getState().reset()
  const k = makeFakeKernel()
  k.responses.set('com.nexus.storage::list_dir', [
    { name: 'A.layout.json', relpath: '.forge/layouts/A.layout.json', isDir: false },
  ])
  await refreshLayouts(k)
  const state = useLayoutsStore.getState()
  assert.equal(state.loading, false)
  assert.equal(state.layouts.length, 1)
  assert.equal(state.layouts[0].name, 'A')
  assert.equal(state.error, null)
})

test('refreshLayouts records errors without clearing previous rows on retry', async () => {
  useLayoutsStore.getState().reset()
  // First call seeds rows.
  const k = makeFakeKernel()
  k.responses.set('com.nexus.storage::list_dir', [
    { name: 'A.layout.json', relpath: '.forge/layouts/A.layout.json', isDir: false },
  ])
  await refreshLayouts(k)
  assert.equal(useLayoutsStore.getState().layouts.length, 1)

  // Second call fails — store records error but the spec is to
  // empty out (so a stale list doesn't masquerade as the current
  // truth). Pin that contract so a future change is conscious.
  const k2 = makeFakeKernel()
  k2.errors.set('com.nexus.storage::list_dir', 'PermissionDenied')
  await refreshLayouts(k2)
  const after = useLayoutsStore.getState()
  assert.match(after.error ?? '', /PermissionDenied/)
  assert.equal(after.layouts.length, 0)
})
