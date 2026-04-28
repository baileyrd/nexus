// shell/vite.sandbox-runtime-plugin.ts
//
// F-8.1.1-fo1 — Vite plugin that bundles the sandbox guest runtime
// (`@nexus/extension-api/sandbox/runtime`) into a self-contained ESM
// string and exposes it as the virtual module `virtual:sandbox-runtime`.
//
// Why this exists
// ───────────────
// The iframe sandbox's srcdoc dynamic-imports the runtime via a URL
// passed in from the host (see `shell/src/host/sandbox/SandboxOrchestrator.ts`
// `buildSandboxSrcDoc` + `shell/src/main.tsx` `getRuntimeUrl`). The
// iframe runs at a null origin and has no module resolver for bare
// specifiers, so `import { bootstrapSandboxedPlugin } from
// '@nexus/extension-api'` cannot work inside the iframe.
//
// This plugin runs esbuild against `runtime.ts` at Vite-build time,
// inlines the only runtime-import dependency (`./protocol.ts`) into the
// output, and hands the shell a string that can be Blob-wrapped + given
// to the iframe as a `blob:` URL it can dynamic-import without any
// resolver.
//
// Contract
// ────────
// Importing `virtual:sandbox-runtime` yields a single default export of
// type `string` — the bundled ESM source. The host blob-wraps this
// string at boot once; every sandboxed plugin re-uses the same blob URL.

import { build } from 'esbuild'
import path from 'path'
import { fileURLToPath } from 'url'
import type { Plugin } from 'vite'

const VIRTUAL_ID = 'virtual:sandbox-runtime'
const RESOLVED_ID = '\0' + VIRTUAL_ID

/**
 * Resolve the absolute path of the runtime entry. Kept exported so the
 * unit test can pin to the same source file.
 */
export function resolveRuntimeEntry(): string {
  const here = path.dirname(fileURLToPath(import.meta.url))
  return path.resolve(
    here,
    '..',
    'packages',
    'nexus-extension-api',
    'src',
    'sandbox',
    'runtime.ts',
  )
}

/**
 * Bundle `runtime.ts` into a self-contained ESM string. Exported for
 * the unit test so the bundling invariants (single `export function
 * bootstrapSandboxedPlugin`, no bare-specifier `import`s) can be
 * asserted without standing up Vite.
 */
export async function bundleSandboxRuntime(opts?: {
  entry?: string
  minify?: boolean
}): Promise<string> {
  const entry = opts?.entry ?? resolveRuntimeEntry()
  const result = await build({
    entryPoints: [entry],
    bundle: true,
    write: false,
    format: 'esm',
    target: 'es2021',
    platform: 'browser',
    // The iframe has no console of its own that we want to suppress —
    // keep `console.error` calls in `runtime.ts` intact so crashes are
    // visible in the browser devtools when a host opens the iframe.
    minify: opts?.minify ?? false,
    legalComments: 'none',
    // No `define` for `process.env.NODE_ENV` — the runtime is a pure
    // protocol shim and reads no globals beyond `globalThis` /
    // `window.parent`.
  })
  if (result.errors.length > 0) {
    const messages = result.errors.map((e) => e.text).join('\n')
    throw new Error(
      `[sandbox-runtime] esbuild bundle failed:\n${messages}`,
    )
  }
  if (result.outputFiles.length !== 1) {
    throw new Error(
      `[sandbox-runtime] expected exactly 1 output file, got ` +
        `${result.outputFiles.length}`,
    )
  }
  return result.outputFiles[0].text
}

/**
 * Vite plugin factory. Plug into `vite.config.ts`'s `plugins` array.
 *
 * Bundles the runtime once on first load and caches the result for the
 * lifetime of the dev server / build process. In dev mode an HMR-like
 * watch is unnecessary because `runtime.ts` rarely changes; restart
 * Vite if it does. Production builds rebundle on every `vite build`.
 */
export function sandboxRuntimePlugin(): Plugin {
  let cached: string | null = null
  return {
    name: 'nexus:sandbox-runtime',
    resolveId(id) {
      if (id === VIRTUAL_ID) return RESOLVED_ID
      return null
    },
    async load(id) {
      if (id !== RESOLVED_ID) return null
      if (cached === null) {
        cached = await bundleSandboxRuntime()
      }
      // Wrap the bundled source as a default-exported string so callers
      // can `import sandboxRuntimeSource from 'virtual:sandbox-runtime'`.
      // `JSON.stringify` is the right escape — the bundle contains
      // backticks and `${}` sequences that would break a template
      // literal.
      return `export default ${JSON.stringify(cached)};\n`
    },
  }
}
