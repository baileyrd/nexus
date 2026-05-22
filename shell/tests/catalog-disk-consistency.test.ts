/**
 * A1 (2026-05-21 gaps audit) regression guard — the shell plugin
 * catalog (`shell/src/plugins/catalog.ts`) and the on-disk plugin
 * directories (`shell/src/plugins/nexus/`) must stay in sync.
 *
 * Two directions, both enforced:
 *
 *   1. **No phantom catalog entries.** Every `import('./nexus/<dir>')`
 *      in the catalog must point at a real file or directory.
 *      TypeScript/Vite resolution would surface this on build, but
 *      dynamic imports are not always checked at lint time — a
 *      dangling entry can sit silently for releases.
 *
 *   2. **No orphan plugin directories.** Every directory under
 *      `shell/src/plugins/nexus/` (except `_lib/`, which is a shared
 *      utility namespace, not a plugin) must be referenced by at
 *      least one catalog `import()`. An orphan dir is dead code that
 *      shows up in source greps and tempts engineers to wire it up
 *      incorrectly.
 *
 * The 2026-05-21 audit found three phantom entries + three orphan
 * dirs + a couple of name mismatches; refactoring during the
 * intervening weeks reconciled all of them via `legacyPluginIds`
 * aliases and renamed imports. This test prevents the state from
 * silently drifting back.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'

const SHELL_DIR = resolve(process.cwd())
const NEXUS_DIR = join(SHELL_DIR, 'src', 'plugins', 'nexus')
const CATALOG_PATH = join(SHELL_DIR, 'src', 'plugins', 'catalog.ts')

const IMPORT_RE = /import\(['"](\.\/nexus\/[^'"]+)['"]\)/g

function extractCatalogImports(): string[] {
  const src = readFileSync(CATALOG_PATH, 'utf8')
  const out: string[] = []
  for (const m of src.matchAll(IMPORT_RE)) {
    out.push(m[1]!)
  }
  return out
}

function topLevelDirFromImport(modPath: string): string {
  // './nexus/foo' → 'foo'; './nexus/graph/globalIndex' → 'graph'
  const parts = modPath.split('/')
  return parts[2]!
}

function resolvesOnDisk(modPath: string): boolean {
  const rel = modPath.replace(/^\.\//, '')
  const base = join(SHELL_DIR, 'src', 'plugins', rel)
  return (
    existsSync(`${base}.ts`) ||
    existsSync(`${base}.tsx`) ||
    existsSync(join(base, 'index.ts')) ||
    existsSync(join(base, 'index.tsx'))
  )
}

test('catalog: every nexus/* import resolves to a real file or index', () => {
  const imports = extractCatalogImports()
  assert.ok(imports.length > 0, 'expected at least one ./nexus/ import in catalog.ts')
  const phantom = imports.filter((p) => !resolvesOnDisk(p))
  assert.deepEqual(
    phantom,
    [],
    `catalog.ts references modules that don't exist on disk:
${phantom.map((p) => `  ${p}`).join('\n')}

Either create the file/dir or remove the catalog entry.`,
  )
})

test('catalog: every nexus/<dir> on disk is referenced by ≥1 catalog import', () => {
  const dirs = readdirSync(NEXUS_DIR, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    // `_lib/` is a shared utility namespace, not a plugin entry.
    .filter((d) => d !== '_lib')

  const importedTopLevel = new Set(extractCatalogImports().map(topLevelDirFromImport))
  const orphans = dirs.filter((d) => !importedTopLevel.has(d))

  assert.deepEqual(
    orphans,
    [],
    `Found plugin directories on disk with no catalog entry:
${orphans.map((d) => `  shell/src/plugins/nexus/${d}/`).join('\n')}

Either wire each into shell/src/plugins/catalog.ts or delete the
directory. See the 2026-05-21 gaps-and-inconsistencies audit A1
for rationale.`,
  )
})
