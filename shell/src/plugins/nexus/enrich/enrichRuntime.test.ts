// shell/src/plugins/nexus/enrich/enrichRuntime.test.ts
//
// BL-045 / FU-12 — runtime coverage for the enrich plugin's inbox-
// scope gate (FU-1) and the propose / apply round-trips. Mirrors the
// recallRuntime.test.ts pattern: stubbed kernel.invoke +
// configuration.getValue so the test exercises the routing logic
// without touching Tauri or a real AI provider.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/enrich/enrichRuntime.test.ts

import { test, beforeEach } from 'node:test'
import assert from 'node:assert/strict'

import {
  applyCustomProposal,
  applyPending,
  filterProposal,
  forceEnrichActiveFile,
  isInInboxScope,
} from './enrichRuntime.ts'
import { useEnrichStore, type EnrichmentProposal } from './enrichStore.ts'
import type { PluginAPI } from '../../../types/plugin.ts'
import { stubPluginAPI } from '../../../testing/typedStubs.ts'

interface InvokeCall {
  pluginId: string
  commandId: string
  args: Record<string, unknown>
}

function reset(): void {
  useEnrichStore.setState({
    pending: new Map(),
    applying: false,
    error: null,
  })
}

function stubApi(opts: {
  inboxPath?: string | null
  tagFiles?: string[]
  tagsThrows?: boolean
  enrichImpl?: (call: InvokeCall) => Promise<unknown>
  applyImpl?: (call: InvokeCall) => Promise<unknown>
  active?: { relpath: string; revision: number } | null
} = {}): {
  api: PluginAPI
  calls: InvokeCall[]
  notifications: Array<{ type: string; message: string }>
} {
  const calls: InvokeCall[] = []
  const inbox = opts.inboxPath
  const tagFiles = opts.tagFiles ?? []
  const notifications: Array<{ type: string; message: string }> = []
  /** Untyped routing core — `invoke<T>` below casts its result to the
   *  production call site's expectation; the test owns the payloads. */
  const route = async (call: InvokeCall): Promise<unknown> => {
    const { pluginId, commandId } = call
    if (pluginId === 'com.nexus.storage' && commandId === 'query_tags') {
      if (opts.tagsThrows) throw new Error('storage offline')
      return tagFiles.map((p) => ({ file_path: p, name: 'inbox', count: 1 }))
    }
    if (pluginId === 'com.nexus.ai' && commandId === 'enrich_file') {
      if (opts.enrichImpl) return opts.enrichImpl(call)
      return null
    }
    if (pluginId === 'com.nexus.ai' && commandId === 'enrich_apply') {
      if (opts.applyImpl) return opts.applyImpl(call)
      return { applied: true }
    }
    return null
  }
  const api = stubPluginAPI({
    kernel: {
      invoke: async <T>(
        pluginId: string,
        commandId: string,
        args?: unknown,
      ): Promise<T> => {
        const call: InvokeCall = {
          pluginId,
          commandId,
          args: (args ?? {}) as Record<string, unknown>,
        }
        calls.push(call)
        return (await route(call)) as T
      },
    },
    configuration: {
      getValue: <T>(key: string, def: T): T => {
        if (key === 'memory.inboxPath') {
          // The test fixes this key's runtime type to string|null; the
          // production call site's T is unresolved here, so the bridge
          // cast is deliberate.
          return inbox === undefined ? def : (inbox as unknown as T)
        }
        return def
      },
    },
    notifications: {
      show: (msg) => {
        notifications.push({ type: msg.type ?? 'info', message: msg.message })
      },
    },
    editor: {
      active: () => opts.active ?? null,
    },
  })
  return { api, calls, notifications }
}

beforeEach(() => {
  reset()
})

// ── isInInboxScope ──────────────────────────────────────────────────────────

test('isInInboxScope: relpath equals memory.inboxPath → true', async () => {
  const { api, calls } = stubApi({ inboxPath: 'Inbox.md' })
  const ok = await isInInboxScope(api, 'Inbox.md')
  assert.equal(ok, true)
  // Path-equality is an early-out — no tag query should fire.
  assert.equal(calls.length, 0)
})

test('isInInboxScope: file in #inbox tag results → true', async () => {
  const { api } = stubApi({ inboxPath: 'Inbox.md', tagFiles: ['notes/today.md'] })
  const ok = await isInInboxScope(api, 'notes/today.md')
  assert.equal(ok, true)
})

test('isInInboxScope: neither match → false (gate closed)', async () => {
  const { api } = stubApi({ inboxPath: 'Inbox.md', tagFiles: ['other.md'] })
  const ok = await isInInboxScope(api, 'random/file.md')
  assert.equal(ok, false)
})

test('isInInboxScope: tag query throws → false (soft fail)', async () => {
  const { api } = stubApi({ inboxPath: 'Inbox.md', tagsThrows: true })
  const ok = await isInInboxScope(api, 'random/file.md')
  assert.equal(ok, false)
})

test('isInInboxScope: no inbox configured + no tag match → false', async () => {
  const { api } = stubApi({ inboxPath: null })
  const ok = await isInInboxScope(api, 'a.md')
  assert.equal(ok, false)
})

// ── forceEnrichActiveFile ───────────────────────────────────────────────────

test('forceEnrichActiveFile: with no active editor warns and skips invoke', async () => {
  const { api, calls, notifications } = stubApi({ active: null })
  await forceEnrichActiveFile(api)
  assert.equal(calls.length, 0)
  assert.equal(notifications[0]?.type, 'warning')
})

test('forceEnrichActiveFile: non-markdown active tab warns and skips invoke', async () => {
  const { api, calls, notifications } = stubApi({
    active: { relpath: 'notes/picture.png', revision: 1 },
  })
  await forceEnrichActiveFile(api)
  assert.equal(calls.length, 0)
  assert.equal(notifications[0]?.type, 'warning')
})

test('forceEnrichActiveFile: bypasses inbox gate and pushes proposal on success', async () => {
  const proposal: EnrichmentProposal = {
    path: 'random/file.md',
    body_hash: 'h',
    tags: ['t'],
    summary: 's',
    related: [],
  }
  const { api, calls } = stubApi({
    inboxPath: 'Inbox.md',
    active: { relpath: 'random/file.md', revision: 1 },
    enrichImpl: async () => proposal,
  })
  await forceEnrichActiveFile(api)
  assert.equal(calls.length, 1)
  assert.equal(calls[0].commandId, 'enrich_file')
  const head = useEnrichStore.getState().pending.values().next().value
  assert.deepEqual(head, proposal)
})

// ── applyPending: head proposal popped on success ───────────────────────────

test('applyPending: pops only the head proposal when many are queued', async () => {
  const a: EnrichmentProposal = {
    path: 'a.md', body_hash: 'a', tags: [], summary: '', related: [],
  }
  const b: EnrichmentProposal = {
    path: 'b.md', body_hash: 'b', tags: [], summary: '', related: [],
  }
  useEnrichStore.getState().setPending(a)
  useEnrichStore.getState().setPending(b)
  assert.equal(useEnrichStore.getState().pending.size, 2)

  const { api } = stubApi({})
  await applyPending(api)

  const after = useEnrichStore.getState().pending
  assert.equal(after.size, 1, 'only the head should be removed')
  assert.ok(after.has('b.md'), 'second proposal must survive')
  assert.ok(!after.has('a.md'), 'head proposal must be removed')
})

test('applyPending: stores reason on rejection and keeps proposal queued', async () => {
  const p: EnrichmentProposal = {
    path: 'a.md', body_hash: 'a', tags: [], summary: '', related: [],
  }
  useEnrichStore.getState().setPending(p)

  const { api } = stubApi({
    applyImpl: async () => ({ applied: false, reason: 'body drift' }),
  })
  await applyPending(api)

  const state = useEnrichStore.getState()
  assert.equal(state.pending.size, 1, 'rejected proposal must remain')
  assert.equal(state.error, 'body drift')
  assert.equal(state.applying, false)
})

// ── AIG-06 — per-field filterProposal ───────────────────────────────

test('filterProposal "all" passes the proposal through unchanged', () => {
  const p: EnrichmentProposal = {
    path: 'a.md',
    body_hash: 'h',
    tags: ['t1', 't2'],
    summary: 's',
    related: ['[[r]]'],
  }
  assert.deepEqual(filterProposal(p, 'all'), p)
})

test('filterProposal "tags" zeroes summary + related, keeps tags', () => {
  const p: EnrichmentProposal = {
    path: 'a.md',
    body_hash: 'h',
    tags: ['t1', 't2'],
    summary: 'will-be-cleared',
    related: ['[[r]]'],
  }
  const filtered = filterProposal(p, 'tags')
  assert.deepEqual(filtered.tags, ['t1', 't2'])
  assert.equal(filtered.summary, '')
  assert.deepEqual(filtered.related, [])
  // Identity: path + body_hash always preserved.
  assert.equal(filtered.path, 'a.md')
  assert.equal(filtered.body_hash, 'h')
})

test('filterProposal "summary" zeroes tags + related, keeps summary', () => {
  const p: EnrichmentProposal = {
    path: 'a.md',
    body_hash: 'h',
    tags: ['t1'],
    summary: 'keep me',
    related: ['[[r]]'],
  }
  const filtered = filterProposal(p, 'summary')
  assert.deepEqual(filtered.tags, [])
  assert.equal(filtered.summary, 'keep me')
  assert.deepEqual(filtered.related, [])
})

test('filterProposal "related" zeroes tags + summary, keeps related', () => {
  const p: EnrichmentProposal = {
    path: 'a.md',
    body_hash: 'h',
    tags: ['t1'],
    summary: 's',
    related: ['[[r1]]', '[[r2]]'],
  }
  const filtered = filterProposal(p, 'related')
  assert.deepEqual(filtered.tags, [])
  assert.equal(filtered.summary, '')
  assert.deepEqual(filtered.related, ['[[r1]]', '[[r2]]'])
})

// ── AIG-06 follow-up — applyCustomProposal partial-tag flow ─────────

test('applyCustomProposal: forwards the caller-built proposal verbatim', async () => {
  const head: EnrichmentProposal = {
    path: 'a.md',
    body_hash: 'h',
    tags: ['keep1', 'drop', 'keep2'],
    summary: 'unchanged',
    related: [],
  }
  useEnrichStore.getState().setPending(head)

  const { api, calls } = stubApi({})
  const customProposal: EnrichmentProposal = {
    ...head,
    tags: ['keep1', 'keep2'],
  }
  await applyCustomProposal(api, customProposal, 'Applied tags to')

  const apply = calls.find((c) => c.commandId === 'enrich_apply')
  assert.ok(apply)
  assert.deepEqual(
    (apply!.args.proposal as EnrichmentProposal).tags,
    ['keep1', 'keep2'],
  )
  // Head was popped because the kernel reported `applied: true` (stub default).
  assert.equal(useEnrichStore.getState().pending.size, 0)
})

test('applyCustomProposal: rejection keeps the head proposal queued', async () => {
  const head: EnrichmentProposal = {
    path: 'a.md',
    body_hash: 'h',
    tags: ['t1'],
    summary: 's',
    related: [],
  }
  useEnrichStore.getState().setPending(head)

  const { api } = stubApi({
    applyImpl: async () => ({ applied: false, reason: 'drift' }),
  })
  await applyCustomProposal(api, { ...head, tags: [] }, 'Enriched')

  const state = useEnrichStore.getState()
  assert.equal(state.pending.size, 1)
  assert.equal(state.error, 'drift')
  assert.equal(state.applying, false)
})

test('applyCustomProposal: no-op when no head proposal is queued', async () => {
  const { api, calls } = stubApi({})
  await applyCustomProposal(
    api,
    {
      path: 'ghost.md',
      body_hash: 'gone',
      tags: [],
      summary: '',
      related: [],
    },
    'Enriched',
  )
  assert.equal(calls.length, 0, 'no enrich_apply call when there is no head')
})
