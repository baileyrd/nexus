/**
 * WI-14 — tests for the legacy-→-new persistence migration script
 * (`scripts/migrate-shell-state.ts`).
 *
 * Lives under `shell/tests/` so the default `pnpm --filter nexus-shell
 * test` glob (`tests/*.test.ts` run via `node --import tsx`) picks it
 * up. The implementation lives in `scripts/` because it's a one-shot
 * standalone tool, not part of the shell runtime — but we want CI to
 * exercise it on every shell-test run since it depends on the new
 * shell's persisted-state shape staying in sync.
 *
 * Two cases minimum (per WI-14 spec):
 *   1. happy path — representative legacy fixture round-trips to the
 *      expected new fixture.
 *   2. missing fields — legacy state with optional fields absent /
 *      `migrate({})` → new shell's empty default, no throw.
 *
 * Plus extras that defend the contract:
 *   - idempotent rerun on already-migrated state (no second write).
 *   - dropped-field provenance: layouts/forgeState/lastPresetId from
 *     the legacy blob don't leak into the new blob.
 *   - dedupe + cap on recentForgePaths.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, writeFileSync, statSync, existsSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'

import {
  migrate,
  emptyNewShellState,
  run,
  type LegacyShellState,
  type NewShellState,
} from '../../scripts/migrate-shell-state.ts'

const FIXTURE_DIR = join(__dirname, '..', '..', 'scripts', '__fixtures__')

function makeTmpDir(prefix: string): string {
  return mkdtempSync(join(tmpdir(), prefix))
}

test('migrate(): happy path — representative legacy fixture → expected new shape', () => {
  const legacyRaw = readFileSync(join(FIXTURE_DIR, 'legacy-layout-state.json'), 'utf8')
  const expectedRaw = readFileSync(join(FIXTURE_DIR, 'expected-shell-state.json'), 'utf8')

  const legacy = JSON.parse(legacyRaw) as LegacyShellState
  const expected = JSON.parse(expectedRaw) as NewShellState

  const actual = migrate(legacy)
  assert.deepEqual(actual, expected)
})

test('migrate(): missing optional fields → empty default state, no throw', () => {
  const empty = migrate({})
  assert.deepEqual(empty, emptyNewShellState())

  // Null / undefined arg also resolves to default.
  assert.deepEqual(migrate(null), emptyNewShellState())
  assert.deepEqual(migrate(undefined), emptyNewShellState())
})

test('migrate(): partial input — only lastForgePath set', () => {
  const out = migrate({ lastForgePath: '/some/forge' })
  assert.equal(out.lastForgePath, '/some/forge')
  assert.deepEqual(out.recentForgePaths, [])
  assert.equal(out.version, 1)
})

test('migrate(): dropped fields do not leak into output', () => {
  // The new ShellState has only three keys; layouts / forgeState /
  // lastPresetId from the legacy blob must not survive migration.
  const out = migrate({
    lastPresetId: 'vibe',
    layouts: { foo: { leftSidePanelCollapsed: true } },
    forgeState: { '/x': { expandedPaths: ['a'], openFile: 'a/b.md' } },
  })
  assert.deepEqual(Object.keys(out).sort(), [
    'lastForgePath',
    'recentForgePaths',
    'version',
  ])
})

test('migrate(): dedupes and caps recentForgePaths at 8', () => {
  const dups = ['/a', '/b', '/a', '/c', '/b', '/d']
  const out = migrate({ recentForgePaths: dups })
  assert.deepEqual(out.recentForgePaths, ['/a', '/b', '/c', '/d'])

  const overflow = Array.from({ length: 12 }, (_, i) => `/forge-${i}`)
  const capped = migrate({ recentForgePaths: overflow })
  assert.equal(capped.recentForgePaths.length, 8)
  assert.equal(capped.recentForgePaths[0], '/forge-0')
  assert.equal(capped.recentForgePaths[7], '/forge-7')
})

test('migrate(): rejects malformed entries in recentForgePaths', () => {
  // Defensive: if the legacy file was hand-edited, a stray non-string
  // shouldn't crash the migration. Just drop it.
  const out = migrate({
    recentForgePaths: ['/ok', '', '/also-ok', null as unknown as string, 42 as unknown as string],
  })
  assert.deepEqual(out.recentForgePaths, ['/ok', '/also-ok'])
})

test('run(): full round-trip via tmp dirs writes the expected file', () => {
  const inDir = makeTmpDir('nexus-migrate-in-')
  const outDir = makeTmpDir('nexus-migrate-out-')
  const legacyRaw = readFileSync(join(FIXTURE_DIR, 'legacy-layout-state.json'), 'utf8')
  writeFileSync(join(inDir, 'layout-state.json'), legacyRaw, 'utf8')

  const result = run(inDir, outDir)
  assert.equal(result.status, 'migrated')

  const written = JSON.parse(readFileSync(result.outputPath, 'utf8')) as NewShellState
  const expected = JSON.parse(
    readFileSync(join(FIXTURE_DIR, 'expected-shell-state.json'), 'utf8'),
  ) as NewShellState
  assert.deepEqual(written, expected)
})

test('run(): no legacy file → writes default, status no-input', () => {
  const inDir = makeTmpDir('nexus-migrate-in-')
  const outDir = makeTmpDir('nexus-migrate-out-')

  const result = run(inDir, outDir)
  assert.equal(result.status, 'no-input')
  assert.deepEqual(result.output, emptyNewShellState())
  assert.ok(existsSync(result.outputPath))
})

test('run(): rerunning on already-migrated state is idempotent (no second write)', () => {
  const inDir = makeTmpDir('nexus-migrate-in-')
  const outDir = makeTmpDir('nexus-migrate-out-')
  const legacyRaw = readFileSync(join(FIXTURE_DIR, 'legacy-layout-state.json'), 'utf8')
  writeFileSync(join(inDir, 'layout-state.json'), legacyRaw, 'utf8')

  const first = run(inDir, outDir)
  assert.equal(first.status, 'migrated')
  const mtimeFirst = statSync(first.outputPath).mtimeMs

  // Tiny pause to make a re-write detectable on filesystems with
  // coarse mtime granularity (e.g. some macOS / Windows setups).
  const start = Date.now()
  while (Date.now() - start < 5) {
    /* spin briefly so any rewrite would tick mtime */
  }

  const second = run(inDir, outDir)
  assert.equal(second.status, 'idempotent')
  const mtimeSecond = statSync(second.outputPath).mtimeMs
  assert.equal(mtimeSecond, mtimeFirst, 'idempotent rerun must not touch the file')
})

test('run(): corrupt legacy file → falls back to empty default, does not throw', () => {
  const inDir = makeTmpDir('nexus-migrate-in-')
  const outDir = makeTmpDir('nexus-migrate-out-')
  writeFileSync(join(inDir, 'layout-state.json'), '{ not json', 'utf8')

  const result = run(inDir, outDir)
  // Corrupt input is treated like no input — we still write a clean
  // default rather than leaving the user with no shell-state.json at
  // all.
  assert.equal(result.status, 'no-input')
  assert.deepEqual(result.output, emptyNewShellState())
})
