// shell/src/plugins/popoutCompatible.test.ts
//
// DG-25 — ADR 0020 popout-compatibility contract test.
//
// `main.tsx` filters `DEFAULT_ON_PLUGINS` by `entry.popoutCompatible !== false`
// when booting a popout window, so the catalog flag is what the runtime
// actually consults. The plugin's own manifest also declares the flag and is
// the authoritative self-description of the plugin's capability — a popout
// boot trusting the catalog while the manifest says otherwise is the
// "runtime surprise" DG-25 calls out.
//
// We can't dynamic-import the plugin modules here because several pull
// CSS via Vite's `import './foo.css'`, which `node --test` does not
// understand. Instead we read each plugin's source file and grep the
// `popoutCompatible` declaration out of the literal manifest object.
// This keeps the contract test cheap and side-effect-free.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { ALL_PLUGINS, type PluginEntry } from './catalog.ts'

const THIS_DIR = dirname(fileURLToPath(import.meta.url))

const norm = (v: boolean | undefined): boolean => v !== false

// Pulls the relative import specifier out of an arrow factory like
//   () => import('./core/zoom').then(m => m.zoomPlugin)
function extractImportPath(entry: PluginEntry): string {
  const src = entry.load.toString()
  const match = src.match(/import\(['"]([^'"]+)['"]\)/)
  if (!match) {
    throw new Error(
      `cannot extract import path from load() of '${entry.id}': ${src}`,
    )
  }
  return match[1]
}

// Resolves the dynamic-import specifier to a real file on disk. The
// catalog uses bare specifiers without extensions; Vite resolves them
// against `.ts` / `.tsx` / `index.ts` / `index.tsx`. We try the same
// set so the test stays in lock-step with the runtime resolution.
function resolveModuleFile(spec: string): string {
  const base = resolve(THIS_DIR, spec)
  const candidates = [
    `${base}.ts`,
    `${base}.tsx`,
    `${base}/index.ts`,
    `${base}/index.tsx`,
  ]
  for (const candidate of candidates) {
    try {
      readFileSync(candidate)
      return candidate
    } catch {
      // try next
    }
  }
  throw new Error(`module file not found for '${spec}' (tried ${candidates.join(', ')})`)
}

// Returns the manifest's `popoutCompatible` value (or `undefined` if
// absent) parsed out of the source. The manifest is always declared as
// an object literal in the plugin's main module, so a tight regex over
// the manifest block is reliable.
//
// Tolerates whitespace and the field appearing anywhere inside the
// manifest object; rejects accidental matches inside unrelated nested
// objects by only scanning between the first `manifest: {` and its
// matching brace.
function readManifestFlag(file: string): boolean | undefined {
  const text = readFileSync(file, 'utf8')
  const start = text.indexOf('manifest:')
  if (start < 0) {
    throw new Error(`no manifest declaration in ${file}`)
  }
  const openBrace = text.indexOf('{', start)
  if (openBrace < 0) {
    throw new Error(`malformed manifest in ${file}`)
  }
  // Walk braces to find the matching close. The manifest body can
  // include nested objects (e.g. `contributes: { ... }`).
  let depth = 0
  let end = -1
  for (let i = openBrace; i < text.length; i++) {
    const ch = text[i]
    if (ch === '{') depth++
    else if (ch === '}') {
      depth--
      if (depth === 0) {
        end = i
        break
      }
    }
  }
  if (end < 0) {
    throw new Error(`unbalanced manifest braces in ${file}`)
  }
  const body = text.slice(openBrace, end + 1)
  const m = body.match(/popoutCompatible\s*:\s*(true|false)\b/)
  if (!m) return undefined
  return m[1] === 'true'
}

test('every catalog entry agrees with its plugin manifest on popoutCompatible', () => {
  const mismatches: Array<{
    id: string
    catalog: boolean
    manifest: boolean
    file: string
  }> = []
  for (const entry of ALL_PLUGINS) {
    const spec = extractImportPath(entry)
    const file = resolveModuleFile(spec)
    const manifestFlag = norm(readManifestFlag(file))
    const catalogFlag = norm(entry.popoutCompatible)
    if (manifestFlag !== catalogFlag) {
      mismatches.push({ id: entry.id, catalog: catalogFlag, manifest: manifestFlag, file })
    }
  }
  assert.deepEqual(
    mismatches,
    [],
    'catalog popoutCompatible drifted from plugin manifest. Either ' +
      'fix the catalog entry or the plugin manifest so they agree. ' +
      'Drift means a popout boot picks a different plugin set than the ' +
      "plugin's own capability self-declaration (the ADR 0020 'runtime " +
      "surprise' DG-25 calls out). Mismatches: " +
      JSON.stringify(mismatches, null, 2),
  )
})
