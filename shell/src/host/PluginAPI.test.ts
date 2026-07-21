/**
 * F-8.1.2 — host-side `pluginId` binding tests.
 *
 * `buildPluginAPI` is the only ingress point where a string flows into
 * trust-sensitive surfaces (storage namespace, event tagging,
 * `PluginRegistry.track` ownership). These tests lock in:
 *
 *   1. `assertValidPluginId` rejects empty / non-string / colon-bearing
 *      ids — the latter would let `pluginId="a:b"` collide with
 *      `localStorage` keys in the `a` plugin's `plugin:a:` namespace.
 *   2. Two `PluginAPI` instances built with different ids do not share
 *      `localStorage` keys — i.e. `apiA.storage.set('k', 'v')` does
 *      not leak to `apiB.storage.get('k')`.
 *
 * Activity-bar event tagging + registry-ownership scoping are covered
 * end-to-end in the sandbox E2E suite (which exercises the
 * orchestrator's per-plugin `apiFactory` wiring), so they are not
 * duplicated here.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  assertValidPluginId,
  buildPluginAPI,
} from './PluginAPI'
import type { PluginRegistry } from './PluginRegistry'

// Minimal `PluginRegistry` stub — `buildPluginAPI` reaches into
// `commands.register`, `track`, `trackSubscription`, `statusBar`,
// `settingsTabs`, `config`, `keybindings`, etc. on construction or on
// surface invocation. We only need surfaces that storage tests touch
// (none) plus enough no-op pass-throughs that the constructor itself
// doesn't throw.
function makeRegistry(): PluginRegistry {
  const stub: Record<string, unknown> = {
    commands: {
      register: () => {},
      execute: () => undefined,
      all: () => [] as unknown[],
    },
    keybindings: { all: () => [] as unknown[] },
    track: () => {},
    trackSubscription: () => {},
    statusBar: { create: () => ({ dispose: () => {} }) },
    settingsTabs: { register: () => {} },
    config: { register: () => {} },
    getService: () => ({ get: <T,>(_k: string, d: T) => d, set: () => {} }),
    registerService: () => {},
  }
  return stub as unknown as PluginRegistry
}

test('assertValidPluginId rejects an empty string', () => {
  assert.throws(() => assertValidPluginId(''), /must not be empty/)
})

test('assertValidPluginId rejects non-string input', () => {
  assert.throws(() => assertValidPluginId(undefined), /must be a string/)
  assert.throws(() => assertValidPluginId(42), /must be a string/)
  assert.throws(() => assertValidPluginId(null), /must be a string/)
})

test('assertValidPluginId rejects ids containing ":"', () => {
  assert.throws(
    () => assertValidPluginId('foo:bar'),
    /must not contain ':'/,
  )
  // `plugin:foo:` would collide with the storage namespace prefix.
  assert.throws(() => assertValidPluginId(':leading'), /must not contain ':'/)
  assert.throws(() => assertValidPluginId('trailing:'), /must not contain ':'/)
})

test('assertValidPluginId accepts realistic dotted plugin ids', () => {
  // None of these should throw.
  assertValidPluginId('com.nexus.editor')
  assertValidPluginId('community.hello-world')
  assertValidPluginId('plugin-id-with-dashes')
  assertValidPluginId('a')
})

test('buildPluginAPI rejects an empty pluginId', () => {
  assert.throws(
    () => buildPluginAPI(makeRegistry(), { pluginId: '', isCore: false }),
    /must not be empty/,
  )
})

test('buildPluginAPI rejects a colon-bearing pluginId (storage namespace escape)', () => {
  assert.throws(
    () =>
      buildPluginAPI(makeRegistry(), {
        pluginId: 'evil:hijack',
        isCore: false,
      }),
    /must not contain ':'/,
  )
})

test('two PluginAPI instances built with different ids have isolated storage namespaces', () => {
  const reg = makeRegistry()
  const apiA = buildPluginAPI(reg, { pluginId: 'plugin.a', isCore: false })
  const apiB = buildPluginAPI(reg, { pluginId: 'plugin.b', isCore: false })

  // Stub a `localStorage` for the duration of this test. JSDOM is not
  // available here, so we install a minimal in-memory shim and clean
  // it up afterward. The shim implements only the subset
  // `api.storage` touches (getItem/setItem/removeItem/keys).
  const store = new Map<string, string>()
  const shim = {
    getItem: (k: string) => (store.has(k) ? store.get(k)! : null),
    setItem: (k: string, v: string) => store.set(k, v),
    removeItem: (k: string) => store.delete(k),
    get length() {
      return store.size
    },
    key(i: number) {
      return [...store.keys()][i] ?? null
    },
    clear() {
      store.clear()
    },
  }
  // `Object.keys(localStorage)` is what `storage.clear()` walks; the
  // proxy below makes it return the shim's own keys.
  const localStorageProxy = new Proxy(shim, {
    ownKeys: () => [...store.keys()],
    getOwnPropertyDescriptor: (_t, k) =>
      typeof k === 'string' && store.has(k)
        ? { configurable: true, enumerable: true, value: store.get(k) }
        : undefined,
  }) as unknown as Storage

  const g = globalThis as { localStorage?: Storage }
  const original = g.localStorage
  g.localStorage = localStorageProxy
  try {
    apiA.storage.set('shared-key', 'A-wrote-this')
    // Plugin B reading the same logical key MUST see null — the
    // namespaces are `plugin:plugin.a:shared-key` vs.
    // `plugin:plugin.b:shared-key`.
    assert.equal(
      apiB.storage.get('shared-key'),
      null,
      'plugin B leaked plugin A storage',
    )
    // A's own read sees its value.
    assert.equal(apiA.storage.get('shared-key'), 'A-wrote-this')

    // B writes the same logical key with different content; A's view
    // remains untouched.
    apiB.storage.set('shared-key', 'B-wrote-this')
    assert.equal(apiA.storage.get('shared-key'), 'A-wrote-this')
    assert.equal(apiB.storage.get('shared-key'), 'B-wrote-this')

    // A's clear() wipes A's namespace only.
    apiA.storage.clear()
    assert.equal(apiA.storage.get('shared-key'), null)
    assert.equal(apiB.storage.get('shared-key'), 'B-wrote-this')

    // #377/C24 — list() enumerates only the calling plugin's own keys,
    // stripped of the internal `plugin:<id>:` namespace prefix, and can
    // be filtered by a key prefix.
    apiA.storage.set('settings.theme', 'dark')
    apiA.storage.set('settings.font', 'mono')
    apiA.storage.set('cache.foo', '1')
    assert.deepEqual(
      [...apiA.storage.list('settings.')].sort(),
      ['settings.font', 'settings.theme'],
    )
    assert.deepEqual(
      [...apiA.storage.list()].sort(),
      ['cache.foo', 'settings.font', 'settings.theme'],
    )
    assert.deepEqual(
      apiB.storage.list(),
      ['shared-key'],
      "A's keys must not leak into B's list()",
    )
  } finally {
    if (original) g.localStorage = original
    else delete g.localStorage
  }
})

test('built PluginAPI exposes every namespace of the common contract (#187)', () => {
  const api = buildPluginAPI(makeRegistry(), {
    pluginId: 'plugin.contract',
    isCore: false,
  })

  // `pluginId` is host-asserted from `BuildOptions`, mirroring the
  // sandbox tier's handshake-bound id.
  assert.equal(api.pluginId, 'plugin.contract')

  // One entry per `NexusPluginContext` member — keep in lockstep with
  // packages/nexus-extension-api/src/index.ts. The compile-only twin
  // (src/types/contractConformance.test-d.ts) checks the types; this
  // checks the built object actually carries the members at runtime.
  const contractMembers: Record<string, string[]> = {
    commands: ['register', 'execute'],
    kernel: ['invoke', 'on'],
    platform: ['fs', 'dialog', 'window', 'shell', 'net'],
    events: ['on', 'emit'],
    storage: ['get', 'set', 'delete', 'list'],
    notifications: ['show'],
    context: ['set', 'get', 'evaluate'],
    input: ['prompt', 'confirm'],
    uri: ['register'],
    activityBar: ['addItem', 'removeItem'],
    statusBar: ['createItem'],
  }
  for (const [ns, members] of Object.entries(contractMembers)) {
    const surface = (api as unknown as Record<string, Record<string, unknown>>)[
      ns
    ]
    assert.ok(surface, `api.${ns} missing from the built PluginAPI`)
    for (const member of members) {
      assert.ok(
        member in surface,
        `api.${ns}.${member} missing from the built PluginAPI`,
      )
    }
  }
})
