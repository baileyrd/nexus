/**
 * F-8.1.1-fo1 — unit tests for the sandbox runtime bundler.
 *
 * The Vite plugin in `shell/vite.sandbox-runtime-plugin.ts` runs
 * `bundleSandboxRuntime()` at Vite-build time and exposes the result as
 * the virtual module `virtual:sandbox-runtime`. These tests assert the
 * invariants `getRuntimeUrl` (in `shell/src/main.tsx`) relies on:
 *
 *   - The bundle is a single self-contained ESM string.
 *   - It exports `bootstrapSandboxedPlugin` as a named export.
 *   - It contains no bare-specifier `import` statements (would fail to
 *     resolve in the null-origin iframe).
 *   - It inlines `protocol.ts` so the iframe never needs to load it
 *     separately.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  bundleSandboxRuntime,
  resolveRuntimeEntry,
} from '../vite.sandbox-runtime-plugin'

test('resolveRuntimeEntry points at runtime.ts in the extension-api package', () => {
  const entry = resolveRuntimeEntry()
  assert.match(
    entry,
    /packages[\\/]nexus-extension-api[\\/]src[\\/]sandbox[\\/]runtime\.ts$/,
  )
})

test('bundleSandboxRuntime emits a non-empty string', async () => {
  const source = await bundleSandboxRuntime()
  assert.equal(typeof source, 'string')
  assert.ok(source.length > 1000, `bundle suspiciously small: ${source.length} bytes`)
})

test('bundle exports bootstrapSandboxedPlugin', async () => {
  const source = await bundleSandboxRuntime()
  // esbuild emits one of these forms depending on the export style:
  //   export { bootstrapSandboxedPlugin };
  //   export function bootstrapSandboxedPlugin(...) { ... }
  const hasNamedExport =
    /export\s*\{[^}]*\bbootstrapSandboxedPlugin\b[^}]*\}/.test(source) ||
    /export\s+function\s+bootstrapSandboxedPlugin\s*\(/.test(source)
  assert.ok(
    hasNamedExport,
    'bundle does not export bootstrapSandboxedPlugin as a named export',
  )
})

test('bundle has no bare-specifier imports (would fail in null-origin iframe)', async () => {
  const source = await bundleSandboxRuntime()
  // Match all top-level static `import` statements. Bare specifiers
  // like `import foo from "@nexus/extension-api"` have no leading
  // `./` or `/` — those would fail to resolve under a null-origin
  // iframe with no module resolver.
  const imports = [...source.matchAll(/^import\s+[^;]*?from\s+['"]([^'"]+)['"];?/gm)]
  for (const match of imports) {
    const specifier = match[1]
    // Allow relative ('./', '../') and absolute ('/') paths only.
    // Anything else is a bare specifier — fail loud.
    assert.ok(
      specifier.startsWith('./') ||
        specifier.startsWith('../') ||
        specifier.startsWith('/'),
      `bundle has bare-specifier import: ${specifier} (line: ${match[0]})`,
    )
  }
})

test('bundle inlines protocol.ts contents (SANDBOX_PROTOCOL_VERSION is present)', async () => {
  const source = await bundleSandboxRuntime()
  // `SANDBOX_PROTOCOL_VERSION` is the canonical value-typed export of
  // `protocol.ts`. If the bundler kept the import as an external
  // reference instead of inlining, the constant wouldn't appear in
  // the output.
  assert.match(source, /SANDBOX_PROTOCOL_VERSION/)
})

test('bundle preserves the handshake nonce post call to window.parent', async () => {
  // Smoke check: every guest must call `window.parent.postMessage` to
  // initiate the handshake. If esbuild stripped or DCE'd this, the
  // guest would never come up.
  const source = await bundleSandboxRuntime()
  assert.match(source, /window\.parent\.postMessage|parent\.postMessage/)
})

test('bundle is deterministic — second build produces the same output', async () => {
  const a = await bundleSandboxRuntime()
  const b = await bundleSandboxRuntime()
  assert.equal(a, b)
})
