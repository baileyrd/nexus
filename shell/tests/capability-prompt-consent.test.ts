// shell/tests/capability-prompt-consent.test.ts
//
// WI-31 — Consent decision logic, capability mapping, and consent
// runner. Pure-function tests; no React, no Tauri.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Capability } from '@nexus/extension-api'

import {
  parseSemVer,
  isPatchOnlyBump,
  decideConsent,
  parsePriorGrant,
} from '../src/plugins/core/capabilityPrompt/consentLogic'
import {
  capsToKernelStrings,
  kernelStringsToCaps,
} from '../src/plugins/core/capabilityPrompt/capabilityMapping'
import {
  runInstallTimeConsent,
  type ConsentRunnerDeps,
} from '../src/plugins/core/capabilityPrompt/requestConsent'
import {
  useCapabilityPromptStore,
} from '../src/plugins/core/capabilityPrompt/capabilityPromptStore'
import type { CommunityPluginManifest } from '../src/host/communityPluginLoader'

// ── SemVer parsing ───────────────────────────────────────────────────────────

test('parseSemVer accepts x.y.z and strips prerelease', () => {
  assert.deepEqual(parseSemVer('1.2.3'), { major: 1, minor: 2, patch: 3 })
  assert.deepEqual(parseSemVer('0.0.1-alpha'), { major: 0, minor: 0, patch: 1 })
  assert.deepEqual(parseSemVer(' 10.20.30 '), { major: 10, minor: 20, patch: 30 })
  assert.equal(parseSemVer(undefined), null)
  assert.equal(parseSemVer(''), null)
  assert.equal(parseSemVer('not-a-version'), null)
  assert.equal(parseSemVer('1.2'), null)
})

test('isPatchOnlyBump requires same major.minor', () => {
  assert.equal(isPatchOnlyBump('1.2.3', '1.2.0'), true)
  assert.equal(isPatchOnlyBump('1.2.3', '1.2.3'), true)
  assert.equal(isPatchOnlyBump('1.3.0', '1.2.99'), false, 'minor bump re-prompts')
  assert.equal(isPatchOnlyBump('2.0.0', '1.99.99'), false, 'major bump re-prompts')
  // Missing / unparseable conservatively false.
  assert.equal(isPatchOnlyBump('1.2.3', undefined), false)
  assert.equal(isPatchOnlyBump(undefined, '1.2.3'), false)
  assert.equal(isPatchOnlyBump('garbage', '1.2.3'), false)
})

// ── Capability mapping (PascalCase ↔ dotted kernel form) ────────────────────

test('capsToKernelStrings produces dotted forms matching kernel', () => {
  const kernel = capsToKernelStrings([
    'FsRead' as Capability,
    'FsReadExternal' as Capability,
    'ProcessSpawn' as Capability,
    'UiNotify' as Capability,
  ])
  assert.deepEqual(kernel, [
    'fs.read',
    'fs.read.external',
    'process.spawn',
    'ui.notify',
  ])
})

test('kernelStringsToCaps round-trips and drops unknowns', () => {
  const caps = kernelStringsToCaps([
    'fs.read',
    'net.http',
    'bogus.thing', // dropped
    'ipc.call',
  ])
  assert.deepEqual(caps, ['FsRead', 'NetHttp', 'IpcCall'] as Capability[])
})

// ── Consent decision routing ────────────────────────────────────────────────

test('decideConsent: no declared caps → auto-accept', () => {
  const d = decideConsent({
    declared: null,
    currentVersion: '1.0.0',
    prior: { version: '', capabilities: [] },
  })
  assert.equal(d.kind, 'auto-accept')

  const e = decideConsent({
    declared: [],
    currentVersion: '1.0.0',
    prior: { version: '', capabilities: [] },
  })
  assert.equal(e.kind, 'auto-accept')
})

test('decideConsent: only low/medium caps → banner (non-blocking)', () => {
  const d = decideConsent({
    declared: ['UiNotify', 'KvRead', 'FsRead'] as Capability[],
    currentVersion: '1.0.0',
    prior: { version: '', capabilities: [] },
  })
  assert.equal(d.kind, 'banner')
})

test('decideConsent: any high-risk cap → blocking modal', () => {
  const d = decideConsent({
    declared: ['UiNotify', 'NetHttp'] as Capability[],
    currentVersion: '1.0.0',
    prior: { version: '', capabilities: [] },
  })
  assert.equal(d.kind, 'modal')
  if (d.kind === 'modal') {
    assert.equal(d.reason, 'first-install')
    assert.deepEqual(d.previouslyGranted, [])
  }
})

test('decideConsent: patch bump with same high-risk set → auto-accept', () => {
  const d = decideConsent({
    declared: ['NetHttp', 'ProcessSpawn'] as Capability[],
    currentVersion: '1.2.3',
    prior: {
      version: '1.2.0',
      capabilities: ['NetHttp', 'ProcessSpawn'] as Capability[],
    },
  })
  assert.equal(d.kind, 'auto-accept')
  if (d.kind === 'auto-accept') {
    assert.equal(d.reason, 'patch-bump')
  }
})

test('decideConsent: patch bump but new high-risk cap → re-prompt', () => {
  const d = decideConsent({
    // ProcessSpawn is new since prior install.
    declared: ['NetHttp', 'ProcessSpawn'] as Capability[],
    currentVersion: '1.2.3',
    prior: {
      version: '1.2.0',
      capabilities: ['NetHttp'] as Capability[],
    },
  })
  assert.equal(d.kind, 'modal')
  if (d.kind === 'modal') {
    assert.equal(d.reason, 'capability-change')
  }
})

test('decideConsent: minor bump → re-prompt as version-bump (not first-install)', () => {
  const d = decideConsent({
    declared: ['NetHttp'] as Capability[],
    currentVersion: '1.3.0',
    prior: {
      version: '1.2.0',
      capabilities: ['NetHttp'] as Capability[],
    },
  })
  assert.equal(d.kind, 'modal')
  if (d.kind === 'modal') {
    assert.equal(d.reason, 'version-bump')
    assert.deepEqual(d.previouslyGranted, ['NetHttp'] as Capability[])
  }
})

test('decideConsent: major bump → re-prompt as version-bump', () => {
  const d = decideConsent({
    declared: ['NetHttp'] as Capability[],
    currentVersion: '2.0.0',
    prior: {
      version: '1.9.9',
      capabilities: ['NetHttp'] as Capability[],
    },
  })
  assert.equal(d.kind, 'modal')
  if (d.kind === 'modal' && d.kind === 'modal') {
    assert.equal(d.reason, 'version-bump')
  }
})

// ── parsePriorGrant ──────────────────────────────────────────────────────────

test('parsePriorGrant maps kernel strings back to PascalCase', () => {
  const parsed = parsePriorGrant({
    version: '1.0.0',
    capabilities: ['fs.read', 'process.spawn'],
  })
  assert.equal(parsed.version, '1.0.0')
  assert.deepEqual(
    new Set(parsed.capabilities),
    new Set(['FsRead', 'ProcessSpawn']),
  )
})

test('parsePriorGrant treats empty-version as no prior grant', () => {
  const parsed = parsePriorGrant({ version: '', capabilities: [] })
  assert.equal(parsed.version, '')
  assert.deepEqual(parsed.capabilities, [])

  const missing = parsePriorGrant(undefined)
  assert.equal(missing.version, '')
  assert.deepEqual(missing.capabilities, [])
})

// ── Runner: denied plugins are persisted + returned ─────────────────────────

function mkManifest(
  overrides: Partial<CommunityPluginManifest> & Pick<CommunityPluginManifest, 'id'>,
): CommunityPluginManifest {
  return {
    id: overrides.id,
    name: overrides.name ?? overrides.id,
    version: overrides.version ?? '1.0.0',
    main: overrides.main ?? 'index.js',
    enabled: overrides.enabled ?? true,
    description: overrides.description,
    author: overrides.author,
    apiVersion: overrides.apiVersion,
    capabilities: overrides.capabilities,
    dir: overrides.dir ?? `/tmp/${overrides.id}`,
    manifestPath: overrides.manifestPath ?? `/tmp/${overrides.id}/plugin.json`,
  }
}

test('runInstallTimeConsent: denied plugins end up in denied set + registered state', async () => {
  // Reset store.
  useCapabilityPromptStore.setState({
    currentModal: null,
    queue: [],
    banners: [],
    denied: new Set(),
  })

  const writes: Array<{ pluginDir: string; version: string; caps: string[] }> = []
  const deps: ConsentRunnerDeps = {
    getGranted: async () => ({}), // no prior grants
    setGranted: async ({ plugin_dir, version, capabilities }) => {
      writes.push({ pluginDir: plugin_dir, version, caps: capabilities })
    },
  }

  const manifests = [
    mkManifest({
      id: 'com.evil.plugin',
      capabilities: ['NetHttp', 'ProcessSpawn'],
    }),
  ]

  const run = runInstallTimeConsent(manifests, deps)

  // Simulate the user clicking Deny once the modal lands. We poll the
  // store one microtask later so the runner has had a chance to enqueue.
  await new Promise((r) => setTimeout(r, 0))
  const current = useCapabilityPromptStore.getState().currentModal
  assert.ok(current, 'modal should be enqueued for high-risk plugin')
  assert.equal(current!.pluginId, 'com.evil.plugin')
  useCapabilityPromptStore.getState().resolveCurrent(false, [])

  const result = await run
  assert.ok(result.denied.has('com.evil.plugin'))
  assert.equal(result.outcomes.get('com.evil.plugin'), 'denied')
  // An empty grant list should have been persisted at the new version.
  assert.equal(writes.length, 1)
  assert.equal(writes[0].version, '1.0.0')
  assert.deepEqual(writes[0].caps, [])
})

test('runInstallTimeConsent: patch bump silently carries prior grants', async () => {
  useCapabilityPromptStore.setState({
    currentModal: null,
    queue: [],
    banners: [],
    denied: new Set(),
  })

  const writes: Array<{ version: string; caps: string[] }> = []
  const deps: ConsentRunnerDeps = {
    getGranted: async () => ({
      'com.good.plugin': {
        version: '1.2.0',
        capabilities: ['net.http'],
      },
    }),
    setGranted: async ({ version, capabilities }) => {
      writes.push({ version, caps: capabilities })
    },
  }

  const manifests = [
    mkManifest({
      id: 'com.good.plugin',
      version: '1.2.5',
      capabilities: ['NetHttp'],
    }),
  ]

  const result = await runInstallTimeConsent(manifests, deps)
  assert.equal(result.outcomes.get('com.good.plugin'), 'auto')
  assert.equal(useCapabilityPromptStore.getState().currentModal, null)
  // Refresh-write under the new version with the same caps.
  assert.equal(writes.length, 1)
  assert.equal(writes[0].version, '1.2.5')
  assert.deepEqual(writes[0].caps, ['net.http'])
})

test('runInstallTimeConsent: low-risk-only plugin gets banner + auto outcome', async () => {
  useCapabilityPromptStore.setState({
    currentModal: null,
    queue: [],
    banners: [],
    denied: new Set(),
  })

  const deps: ConsentRunnerDeps = {
    getGranted: async () => ({}),
    setGranted: async () => { /* no-op */ },
  }

  const result = await runInstallTimeConsent(
    [
      mkManifest({
        id: 'com.chill.plugin',
        capabilities: ['UiNotify', 'KvRead'],
      }),
    ],
    deps,
  )
  assert.equal(result.outcomes.get('com.chill.plugin'), 'auto')
  assert.equal(result.denied.size, 0)
  assert.equal(useCapabilityPromptStore.getState().banners.length, 1)
  // Explicitly no modal enqueued.
  assert.equal(useCapabilityPromptStore.getState().currentModal, null)
})

test('runInstallTimeConsent: disabled manifests are skipped entirely', async () => {
  useCapabilityPromptStore.setState({
    currentModal: null,
    queue: [],
    banners: [],
    denied: new Set(),
  })

  let getCalls = 0
  const deps: ConsentRunnerDeps = {
    getGranted: async (dirs) => {
      getCalls += 1
      // The runner should NOT have included the disabled plugin.
      assert.equal(Object.keys(dirs).length, 0)
      return {}
    },
    setGranted: async () => { /* no-op */ },
  }

  const result = await runInstallTimeConsent(
    [
      mkManifest({
        id: 'com.off.plugin',
        enabled: false,
        capabilities: ['NetHttp'],
      }),
    ],
    deps,
  )
  // Early-exit path — getCalls should be 0 if the runner skipped
  // everything before the batch call.
  assert.equal(getCalls, 0)
  assert.equal(result.outcomes.size, 0)
  assert.equal(result.denied.size, 0)
})

test('runInstallTimeConsent: approve persists kernel-format strings', async () => {
  useCapabilityPromptStore.setState({
    currentModal: null,
    queue: [],
    banners: [],
    denied: new Set(),
  })

  const writes: Array<{ caps: string[] }> = []
  const deps: ConsentRunnerDeps = {
    getGranted: async () => ({}),
    setGranted: async ({ capabilities }) => {
      writes.push({ caps: capabilities })
    },
  }

  const manifests = [
    mkManifest({
      id: 'com.httpthing.plugin',
      capabilities: ['NetHttp', 'FsRead'],
    }),
  ]

  const run = runInstallTimeConsent(manifests, deps)
  await new Promise((r) => setTimeout(r, 0))
  const current = useCapabilityPromptStore.getState().currentModal
  assert.ok(current)
  // Approve only NetHttp (simulate the user unchecking FsRead, which
  // is medium-risk and in practice non-uncheckable — but the runner
  // must serialise whatever comes back from the modal).
  useCapabilityPromptStore.getState().resolveCurrent(true, [
    'NetHttp' as Capability,
  ])

  const result = await run
  assert.equal(result.outcomes.get('com.httpthing.plugin'), 'approved')
  assert.equal(writes.length, 1)
  assert.deepEqual(writes[0].caps, ['net.http'])
})
