// Tests for the noteContext backlinks loader — the always-on
// subscriber + pure decode logic absorbed from the retired
// `nexus.backlinks` plugin (phase 4.3 merge). Recovers the coverage
// that was dropped when that plugin's colocated tests were deleted.
//
// Two layers:
//   1. `decode` / `basename` — pure kernel-payload shaping.
//   2. `startBacklinksLoader` — event-driven behaviour: which IPC the
//      load path picks, the tab-switch request-id race guard, the
//      kernel-availability + error branches, and workspace-close reset.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { useEditorStore } from '../editor/editorStore.ts'
import {
  basename,
  decode,
  startBacklinksLoader,
} from './backlinksLoader.ts'
import { useBacklinksDataStore } from './backlinksDataStore.ts'
import type { PluginAPI } from '../../../types/plugin.ts'

// ── pure: basename ────────────────────────────────────────────────

test('basename: strips the directory prefix', () => {
  assert.equal(basename('notes/sub/file.md'), 'file.md')
  assert.equal(basename('file.md'), 'file.md')
  assert.equal(basename('a/b/c'), 'c')
})

// ── pure: decode ──────────────────────────────────────────────────

test('decode: non-array input yields an empty list', () => {
  assert.deepEqual(decode(null, 'cur.md'), [])
  assert.deepEqual(decode(undefined, 'cur.md'), [])
  assert.deepEqual(decode({}, 'cur.md'), [])
  assert.deepEqual(decode('nope', 'cur.md'), [])
})

test('decode: skips malformed / non-object items and items with no source_path', () => {
  const raw = [
    null,
    'string',
    42,
    { link_text: 'orphan' }, // no source_path → skipped
    { source_path: 'real.md', link_text: 'ok', link_type: 'wikilink' },
  ]
  const out = decode(raw, 'cur.md')
  assert.equal(out.length, 1)
  assert.equal(out[0].sourceRelpath, 'real.md')
})

test('decode: filters self-references', () => {
  const raw = [
    { source_path: 'cur.md', link_text: 'self' },
    { source_path: 'other.md', link_text: 'inbound' },
  ]
  const out = decode(raw, 'cur.md')
  assert.deepEqual(
    out.map((b) => b.sourceRelpath),
    ['other.md'],
  )
})

test('decode: derives sourceName from the path basename', () => {
  const out = decode([{ source_path: 'a/b/deep.md' }], 'cur.md')
  assert.equal(out[0].sourceName, 'deep.md')
})

test('decode: defaults missing/ill-typed linkText and linkType to empty strings', () => {
  const out = decode(
    [{ source_path: 'x.md', link_text: 123, link_type: null }],
    'cur.md',
  )
  assert.equal(out[0].linkText, '')
  assert.equal(out[0].linkType, '')
})

test('decode: normalises empty / absent fragment to null, keeps non-empty', () => {
  const out = decode(
    [
      { source_path: 'a.md' }, // absent
      { source_path: 'b.md', fragment: '' }, // empty
      { source_path: 'c.md', fragment: '^blk-1' }, // present
      { source_path: 'd.md', fragment: 42 }, // ill-typed
    ],
    'cur.md',
  )
  assert.equal(out[0].fragment, null)
  assert.equal(out[1].fragment, null)
  assert.equal(out[2].fragment, '^blk-1')
  assert.equal(out[3].fragment, null)
})

// ── event-driven: startBacklinksLoader ────────────────────────────
//
// `startBacklinksLoader` wires module-level zustand subscriptions that
// can't be torn down, so we install it exactly once and step through
// the scenarios sequentially inside a single test. Each step resets
// the relevant store state first.

/** Drain pending micro/macrotasks so the loader's awaited kernel
 *  round-trips settle before we assert. */
async function settle(): Promise<void> {
  for (let i = 0; i < 6; i++) await new Promise((r) => setTimeout(r, 0))
}

test('startBacklinksLoader: event-driven load behaviour', async () => {
  // Reset both stores to a clean baseline and make the seed-on-activate
  // microtask a no-op (no active file yet).
  useEditorStore.setState({ activeRelpath: null })
  useBacklinksDataStore.getState().clear()

  // Controllable mock kernel. `invokeImpl` is swapped per scenario;
  // every call is recorded so we can assert which handler ran.
  const calls: Array<{ command: string; args: Record<string, unknown> }> = []
  let available = true
  let invokeImpl: (
    command: string,
    args: Record<string, unknown>,
  ) => Promise<unknown> = async () => []
  const wsClosedHandlers: Array<() => void> = []

  const api = {
    kernel: {
      available: async () => available,
      invoke: async (
        _plugin: string,
        command: string,
        args: Record<string, unknown>,
      ) => {
        calls.push({ command, args })
        return invokeImpl(command, args)
      },
    },
    events: {
      on: (event: string, cb: () => void) => {
        if (event === 'workspace:closed') wsClosedHandlers.push(cb)
        return () => {}
      },
    },
  } as unknown as PluginAPI

  startBacklinksLoader(api)
  await settle()

  // ── 1. Active-file change issues the unfiltered `backlinks` IPC and
  // populates the store with the decoded, self-ref-filtered list.
  invokeImpl = async () => [
    { source_path: 'cur.md', link_text: 'self' }, // filtered
    { source_path: 'other.md', link_text: 'hi', link_type: 'wikilink' },
  ]
  useEditorStore.setState({ activeRelpath: 'cur.md' })
  await settle()
  {
    const s = useBacklinksDataStore.getState()
    assert.equal(s.currentRelpath, 'cur.md')
    assert.equal(s.loading, false)
    assert.equal(s.error, null)
    assert.deepEqual(
      s.links.map((b) => b.sourceRelpath),
      ['other.md'],
    )
    assert.equal(calls.at(-1)?.command, 'backlinks')
    assert.deepEqual(calls.at(-1)?.args, { path: 'cur.md' })
  }

  // ── 2. Setting a block filter re-issues against `backlinks_to_block`
  // with the block id (and no leading `^`).
  invokeImpl = async () => [{ source_path: 'narrowed.md' }]
  useBacklinksDataStore.getState().setBlockFilter('blk-1')
  await settle()
  {
    const s = useBacklinksDataStore.getState()
    assert.equal(calls.at(-1)?.command, 'backlinks_to_block')
    assert.deepEqual(calls.at(-1)?.args, { path: 'cur.md', block_id: 'blk-1' })
    assert.deepEqual(
      s.links.map((b) => b.sourceRelpath),
      ['narrowed.md'],
    )
  }

  // ── 3. Switching files clears the active block filter and reverts to
  // the unfiltered handler.
  invokeImpl = async () => []
  useEditorStore.setState({ activeRelpath: 'next.md' })
  await settle()
  {
    const s = useBacklinksDataStore.getState()
    assert.equal(s.blockFilter, null, 'block filter cleared on file switch')
    assert.equal(calls.at(-1)?.command, 'backlinks')
    assert.deepEqual(calls.at(-1)?.args, { path: 'next.md' })
  }

  // ── 4. Tab-switch race: a slow response for the file we just left
  // must not overwrite the newer file's data.
  let resolveSlow: ((v: unknown) => void) | null = null
  invokeImpl = (command, args) => {
    if (args.path === 'slow.md') {
      return new Promise((r) => {
        resolveSlow = r
      })
    }
    return Promise.resolve([{ source_path: 'fast-inbound.md' }])
  }
  useEditorStore.setState({ activeRelpath: 'slow.md' }) // hangs in flight
  useEditorStore.setState({ activeRelpath: 'fast.md' }) // supersedes it
  await settle()
  // Now let the stale 'slow.md' response land late.
  resolveSlow?.([{ source_path: 'STALE.md' }])
  await settle()
  {
    const s = useBacklinksDataStore.getState()
    assert.equal(s.currentRelpath, 'fast.md')
    assert.deepEqual(
      s.links.map((b) => b.sourceRelpath),
      ['fast-inbound.md'],
      'stale response for the previous file must be dropped',
    )
  }

  // ── 5. Kernel unavailable → surface a not-ready error, stop loading.
  available = false
  useEditorStore.setState({ activeRelpath: 'unready.md' })
  await settle()
  {
    const s = useBacklinksDataStore.getState()
    assert.equal(s.loading, false)
    assert.equal(s.error, 'Kernel not ready.')
    assert.deepEqual(s.links, [])
  }
  available = true

  // ── 6. invoke rejects → error message captured, links cleared.
  invokeImpl = async () => {
    throw new Error('boom')
  }
  useEditorStore.setState({ activeRelpath: 'broken.md' })
  await settle()
  {
    const s = useBacklinksDataStore.getState()
    assert.equal(s.loading, false)
    assert.equal(s.error, 'boom')
    assert.deepEqual(s.links, [])
  }

  // ── 7. workspace:closed clears the store.
  invokeImpl = async () => [{ source_path: 'whatever.md' }]
  useEditorStore.setState({ activeRelpath: 'live.md' })
  await settle()
  assert.notEqual(useBacklinksDataStore.getState().currentRelpath, null)
  for (const cb of wsClosedHandlers) cb()
  {
    const s = useBacklinksDataStore.getState()
    assert.equal(s.currentRelpath, null)
    assert.deepEqual(s.links, [])
    assert.equal(s.error, null)
    assert.equal(s.blockFilter, null)
  }
})
