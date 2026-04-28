/**
 * BL-008 — community.mermaid plugin lifecycle tests.
 *
 * Stays at the plugin-API contract layer: asserts `activate(api)` calls
 * `api.editor.registerFencedCodeRenderer('mermaid', ...)` and that the
 * disposer returned from registration is invoked on `deactivate`. Does
 * NOT actually import mermaid — node `--test` runs without jsdom and
 * mermaid's render path drives DOM APIs heavily, so the dynamic-import
 * path is exercised manually via the e2e suite (`pnpm e2e`).
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { mermaidPlugin } from '../src/plugins/community/mermaid/index.ts'
import type { PluginAPI, FencedRenderer } from '../src/types/plugin'

interface RegisteredCall {
  language: string
  renderer: FencedRenderer
}

interface Harness {
  api: PluginAPI
  registered: RegisteredCall[]
  disposeCount(): number
}

function fakePluginAPI(): Harness {
  const registered: RegisteredCall[] = []
  let disposed = 0
  const stub = {} as PluginAPI
  stub.editor = {
    active: () => null,
    onChange: () => () => {},
    registerFencedCodeRenderer(language: string, renderer: FencedRenderer) {
      registered.push({ language, renderer })
      return () => {
        disposed++
      }
    },
  }
  return {
    api: stub,
    registered,
    disposeCount: () => disposed,
  }
}

test('mermaid plugin manifest is community.mermaid v1', () => {
  assert.equal(mermaidPlugin.manifest.id, 'community.mermaid')
  assert.equal(mermaidPlugin.manifest.core, false)
  assert.equal(mermaidPlugin.manifest.apiVersion, 1)
})

test('mermaid plugin activate registers `mermaid` fenced renderer', async () => {
  const harness = fakePluginAPI()
  await mermaidPlugin.activate(harness.api)
  assert.equal(harness.registered.length, 1)
  assert.equal(harness.registered[0]!.language, 'mermaid')
  assert.equal(typeof harness.registered[0]!.renderer, 'function')
  await mermaidPlugin.deactivate?.()
  assert.equal(harness.disposeCount(), 1, 'disposer fired on deactivate')
})

test('mermaid plugin deactivate is idempotent', async () => {
  const harness = fakePluginAPI()
  await mermaidPlugin.activate(harness.api)
  await mermaidPlugin.deactivate?.()
  await mermaidPlugin.deactivate?.()
  assert.equal(harness.disposeCount(), 1, 'second deactivate is a no-op')
})
