// shell/src/plugins/core/capabilityPrompt/applyCapabilityChange.test.ts
//
// BL-096 follow-up — unit tests for the helper that wires the
// "user changed the cap selection" UI flow to both the persisted-
// disk write (`set_plugin_granted_capabilities`) and the new live
// kernel-side revoke (`revoke_plugin_capability`).

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  applyCapabilityChange,
  diffRevokedCapabilities,
  type ApplyCapabilityInvoker,
} from './applyCapabilityChange.ts'
import type { Capability } from '@nexus/extension-api'

interface RecordedInvoke {
  command: string
  args: unknown
}

function makeRecordingInvoker(
  responses: Partial<Record<string, () => unknown | Promise<unknown>>> = {},
): {
  invoker: ApplyCapabilityInvoker
  calls: RecordedInvoke[]
} {
  const calls: RecordedInvoke[] = []
  const invoker: ApplyCapabilityInvoker = {
    invoke: async <T = unknown>(command: string, args?: unknown): Promise<T> => {
      calls.push({ command, args })
      const handler = responses[command]
      if (handler) {
        const out = await handler()
        return out as T
      }
      return undefined as T
    },
  }
  return { invoker, calls }
}

// ── diffRevokedCapabilities ────────────────────────────────────────────────

test('diffRevokedCapabilities: returns prior caps not in next', () => {
  const fsRead: Capability = 'FsRead'
  const fsWrite: Capability = 'FsWrite'
  const netHttp: Capability = 'NetHttp'
  const removed = diffRevokedCapabilities(
    [fsRead, fsWrite, netHttp],
    [fsRead],
  )
  assert.equal(removed.length, 2)
  assert.ok(removed.includes('FsWrite'))
  assert.ok(removed.includes('NetHttp'))
})

test('diffRevokedCapabilities: empty prior → empty diff', () => {
  const fsRead: Capability = 'FsRead'
  assert.deepEqual(diffRevokedCapabilities([], [fsRead]), [])
})

test('diffRevokedCapabilities: identical sets → empty diff', () => {
  const fsRead: Capability = 'FsRead'
  const fsWrite: Capability = 'FsWrite'
  assert.deepEqual(
    diffRevokedCapabilities([fsRead, fsWrite], [fsRead, fsWrite]),
    [],
  )
})

test('diffRevokedCapabilities: prior with no next → every prior is removed', () => {
  const fsRead: Capability = 'FsRead'
  const fsWrite: Capability = 'FsWrite'
  const out = diffRevokedCapabilities([fsRead, fsWrite], [])
  assert.equal(out.length, 2)
})

// ── applyCapabilityChange ──────────────────────────────────────────────────

test('applyCapabilityChange: writes the file then revokes removed caps in order', async () => {
  const fsRead: Capability = 'FsRead'
  const netHttp: Capability = 'NetHttp'
  const { invoker, calls } = makeRecordingInvoker()
  const result = await applyCapabilityChange(invoker, {
    pluginId: 'community.example',
    pluginDir: '/plugins/example',
    version: '1.2.3',
    prior: [fsRead, netHttp],
    next: [fsRead],
  })
  // Order: file write first, then revoke for each removed cap.
  assert.equal(calls[0]!.command, 'set_plugin_granted_capabilities')
  assert.equal(calls[1]!.command, 'revoke_plugin_capability')
  assert.equal(calls.length, 2)
  // Persist payload uses kernel-string form.
  assert.deepEqual((calls[0]!.args as { capabilities: string[] }).capabilities, [
    'fs.read',
  ])
  // Revoke payload uses kernel-string form.
  assert.deepEqual(calls[1]!.args, {
    pluginId: 'community.example',
    capability: 'net.http',
  })
  assert.equal(result.revoked.length, 1)
  assert.equal(result.revokeErrors.length, 0)
})

test('applyCapabilityChange: next=null persists empty set and revokes every prior cap', async () => {
  const fsRead: Capability = 'FsRead'
  const fsWrite: Capability = 'FsWrite'
  const { invoker, calls } = makeRecordingInvoker()
  const result = await applyCapabilityChange(invoker, {
    pluginId: 'community.example',
    pluginDir: '/plugins/example',
    version: '1.0.0',
    prior: [fsRead, fsWrite],
    next: null,
  })
  // 1 file write + 2 revokes.
  assert.equal(calls.length, 3)
  assert.equal(calls[0]!.command, 'set_plugin_granted_capabilities')
  assert.deepEqual(
    (calls[0]!.args as { capabilities: string[] }).capabilities,
    [],
  )
  assert.equal(result.revoked.length, 2)
})

test('applyCapabilityChange: identical prior + next → file write only, zero revokes', async () => {
  const fsRead: Capability = 'FsRead'
  const { invoker, calls } = makeRecordingInvoker()
  const result = await applyCapabilityChange(invoker, {
    pluginId: 'community.example',
    pluginDir: '/plugins/example',
    version: '1.0.0',
    prior: [fsRead],
    next: [fsRead],
  })
  assert.equal(calls.length, 1)
  assert.equal(calls[0]!.command, 'set_plugin_granted_capabilities')
  assert.equal(result.revoked.length, 0)
})

test('applyCapabilityChange: a failing revoke is captured but does not abort siblings', async () => {
  const fsRead: Capability = 'FsRead'
  const fsWrite: Capability = 'FsWrite'
  let revokeIdx = 0
  const { invoker, calls } = makeRecordingInvoker({
    revoke_plugin_capability: () => {
      revokeIdx += 1
      if (revokeIdx === 1) throw new Error('plugin not loaded')
      return undefined
    },
  })
  const result = await applyCapabilityChange(invoker, {
    pluginId: 'community.example',
    pluginDir: '/plugins/example',
    version: '1.0.0',
    prior: [fsRead, fsWrite],
    next: [],
  })
  // Two revoke attempts, first fails, second succeeds.
  assert.equal(calls.filter((c) => c.command === 'revoke_plugin_capability').length, 2)
  assert.equal(result.revoked.length, 2)
  assert.equal(result.revokeErrors.length, 1)
  assert.match(String(result.revokeErrors[0]?.error), /plugin not loaded/)
})

test('applyCapabilityChange: a failing file write throws (caller catches)', async () => {
  const fsRead: Capability = 'FsRead'
  const { invoker, calls } = makeRecordingInvoker({
    set_plugin_granted_capabilities: () => {
      throw new Error('disk full')
    },
  })
  await assert.rejects(
    () =>
      applyCapabilityChange(invoker, {
        pluginId: 'community.example',
        pluginDir: '/plugins/example',
        version: '1.0.0',
        prior: [fsRead],
        next: [],
      }),
    /disk full/,
  )
  // No revoke attempted after the persist failed.
  assert.equal(calls.length, 1)
})
