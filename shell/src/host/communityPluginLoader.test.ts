// shell/src/host/communityPluginLoader.test.ts
//
// WI-33 — Shell-side api_version check tests.
//
// Sibling-of-implementation; surfaced to the default `pnpm test` glob
// via `tests/api-version-check.test.ts` (mirrors the ExtensionHost +
// UriHandlerRegistry shim pattern).
//
// Coverage (matches Phase 3a WI-33 spec):
//   - apiVersion === PLUGIN_API_VERSION       → ok
//   - apiVersion === PLUGIN_API_VERSION + 1   → rejected (future)
//   - apiVersion === 0                         → rejected (past)
//   - apiVersion undefined                     → warn-continue (legacy)
//
// The kernel-side equivalent is tested at
// `crates/nexus-plugins/src/loader.rs:1996-2014`. This file exists so
// the shell surface — the mirror Check that runs BEFORE the JS bundle
// is dynamic-imported — has its own coverage without depending on the
// Rust test harness.

// @ts-expect-error tsc lib doesn't include node builtins
import { test, beforeEach } from 'node:test'
// @ts-expect-error tsc lib doesn't include node builtins
import assert from 'node:assert/strict'
import { PLUGIN_API_VERSION } from '@nexus/extension-api'
import {
  PluginApiVersionError,
  checkApiVersion,
  __resetLegacyWarnMemoForTests,
} from './communityPluginLoader.ts'

beforeEach(() => {
  __resetLegacyWarnMemoForTests()
})

test('matching apiVersion is accepted', () => {
  const v = checkApiVersion('com.example.p', PLUGIN_API_VERSION)
  assert.equal(v.ok, true)
})

test('future apiVersion is rejected with PluginApiVersionError', () => {
  const v = checkApiVersion('com.example.p', PLUGIN_API_VERSION + 1)
  assert.equal(v.ok, false)
  if (v.ok) return
  assert.ok(v.error instanceof PluginApiVersionError)
  assert.equal(v.error.kind, 'api_version_mismatch')
  assert.equal(v.error.pluginId, 'com.example.p')
  assert.equal(v.error.requested, PLUGIN_API_VERSION + 1)
  assert.equal(v.error.supported, PLUGIN_API_VERSION)
  // Message must include both sides of the mismatch so the dev-console
  // log a dropped plugin produces is self-describing.
  assert.ok(v.error.message.includes(String(PLUGIN_API_VERSION + 1)))
  assert.ok(v.error.message.includes(String(PLUGIN_API_VERSION)))
})

test('past apiVersion (0) is rejected', () => {
  const v = checkApiVersion('com.example.p', 0)
  assert.equal(v.ok, false)
  if (v.ok) return
  assert.equal(v.error.requested, 0)
  assert.equal(v.error.supported, PLUGIN_API_VERSION)
})

test('undefined apiVersion is warn-continue, not a hard reject', () => {
  // Capture console.warn so we can assert the one-shot legacy warning fires
  // exactly once, then not again on re-check of the same plugin id.
  const originalWarn = console.warn
  const warnings: string[] = []
  console.warn = (...args: unknown[]) => {
    warnings.push(args.map(String).join(' '))
  }
  try {
    const v1 = checkApiVersion('com.example.legacy', undefined)
    const v2 = checkApiVersion('com.example.legacy', undefined)
    assert.equal(v1.ok, true)
    assert.equal(v2.ok, true)
    assert.equal(
      warnings.length,
      1,
      'legacy-plugin warn must only fire once per plugin id',
    )
    assert.ok(warnings[0].includes('com.example.legacy'))
    assert.ok(warnings[0].includes('legacy plugin'))
  } finally {
    console.warn = originalWarn
  }
})

test('distinct legacy plugin ids warn independently', () => {
  const originalWarn = console.warn
  const warnings: string[] = []
  console.warn = (...args: unknown[]) => {
    warnings.push(args.map(String).join(' '))
  }
  try {
    checkApiVersion('com.example.a', undefined)
    checkApiVersion('com.example.b', undefined)
    assert.equal(warnings.length, 2)
  } finally {
    console.warn = originalWarn
  }
})

test('explicit supported override is respected (for future major-bump testing)', () => {
  // Pretend the shell is on version 2 — a plugin targeting 1 should now
  // be rejected, and a plugin targeting 2 should pass. This keeps the
  // test valid once the shell bumps PLUGIN_API_VERSION without having
  // to edit this file.
  const pretendSupported = 2
  const v1 = checkApiVersion('com.example.p', 1, pretendSupported)
  const v2 = checkApiVersion('com.example.p', 2, pretendSupported)
  assert.equal(v1.ok, false)
  assert.equal(v2.ok, true)
  if (v1.ok) return
  assert.equal(v1.error.requested, 1)
  assert.equal(v1.error.supported, 2)
})
