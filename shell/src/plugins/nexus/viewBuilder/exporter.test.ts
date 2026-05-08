// shell/src/plugins/nexus/viewBuilder/exporter.test.ts
//
// BL-067 Phase 2d — unit tests for the layout-as-plugin exporter.
//
// Covers the slug / camelCase helpers, the four file-content
// renderers (manifest.toml + index.ts + layout.json + README), and the
// storage-fronted `writeExportedPlugin` against a fake `KernelAPI`.
// Mirrors the layoutsStore.test pattern (single-file fake kernel,
// node:test runner).

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  buildExportedFiles,
  camelize,
  exportDirRelpath,
  slugify,
  writeExportedPlugin,
} from './exporter.ts'
import type { KernelAPI } from '../../../types/plugin.ts'
import type { WorkspaceJSON } from '../../../workspace/types.ts'

// ── slug / camelize ─────────────────────────────────────────────────────────

test('slugify lowercases, swaps whitespace, strips punctuation', () => {
  assert.equal(slugify('Focus'), 'focus')
  assert.equal(slugify('Research Mode'), 'research-mode')
  assert.equal(slugify('Dev Pair v2'), 'dev-pair-v2')
  assert.equal(slugify('AI/Pair'), 'aipair')
  assert.equal(slugify('  spaced   out  '), 'spaced-out')
  assert.equal(slugify('--leading--'), 'leading')
  assert.equal(slugify('a---b'), 'a-b')
})

test('slugify falls back to "layout" for empty input', () => {
  assert.equal(slugify(''), 'layout')
  assert.equal(slugify('   '), 'layout')
  assert.equal(slugify('!@#$'), 'layout')
})

test('camelize joins parts and protects against digit-leading idents', () => {
  assert.equal(camelize('focus'), 'focus')
  assert.equal(camelize('research-mode'), 'researchMode')
  assert.equal(camelize('dev_pair_v2'), 'devPairV2')
  assert.equal(camelize('a-b-c'), 'aBC')
  assert.equal(camelize('2024-archive'), '_2024Archive')
  assert.equal(camelize(''), 'layout')
})

// ── buildExportedFiles ─────────────────────────────────────────────────────

const SAMPLE_LAYOUT: WorkspaceJSON = {
  main: {
    kind: 'split',
    id: 'm',
    direction: 'horizontal',
    children: [{ kind: 'tabs', id: 't', leaves: [], activeIndex: 0 }],
  },
  left: {
    kind: 'split',
    id: 'l',
    direction: 'vertical',
    children: [{ kind: 'tabs', id: 'lt', leaves: [], activeIndex: 0 }],
    side: 'left',
    collapsed: false,
    size: 280,
  },
  right: {
    kind: 'split',
    id: 'r',
    direction: 'vertical',
    children: [{ kind: 'tabs', id: 'rt', leaves: [], activeIndex: 0 }],
    side: 'right',
    collapsed: false,
    size: 280,
  },
  active: null,
  lastOpenFiles: [],
}

test('buildExportedFiles emits exactly four files', () => {
  const out = buildExportedFiles('Focus', SAMPLE_LAYOUT)
  assert.deepEqual(
    Object.keys(out.files).sort(),
    ['README.md', 'focus.layout.json', 'index.ts', 'manifest.toml'].sort(),
  )
})

test('buildExportedFiles keeps slug + plugin id consistent across files', () => {
  const out = buildExportedFiles('Research Mode', SAMPLE_LAYOUT)
  assert.equal(out.slug, 'research-mode')
  assert.equal(out.pluginId, 'research-mode.layout')
  assert.match(out.files['manifest.toml']!, /id = "research-mode\.layout"/)
  assert.match(out.files['index.ts']!, /id: 'research-mode\.layout'/)
  assert.match(out.files['index.ts']!, /export const researchModeLayoutPlugin/)
  assert.match(out.files['README.md']!, /research-mode\.layout/)
})

test('buildExportedFiles round-trips the layout JSON byte-identically', () => {
  const out = buildExportedFiles('Focus', SAMPLE_LAYOUT)
  const parsed = JSON.parse(out.files['focus.layout.json']!) as WorkspaceJSON
  assert.deepEqual(parsed, SAMPLE_LAYOUT)
})

test('buildExportedFiles escapes quotes / backslashes in TOML metadata', () => {
  const out = buildExportedFiles('Hard "name" \\ with \\quotes', SAMPLE_LAYOUT)
  const manifest = out.files['manifest.toml']!
  // Comment line preserves human form (escaped in the comment too).
  assert.match(manifest, /Hard \\"name\\" \\\\ with \\\\quotes/)
  // The `name = "..."` field should contain escaped quotes + backslashes.
  assert.match(manifest, /name = "Hard \\"name\\" \\\\ with \\\\quotes layout"/)
})

test('buildExportedFiles strips newlines from name fields in TOML', () => {
  const out = buildExportedFiles('Two\nLines', SAMPLE_LAYOUT)
  // A multi-line value in a single-quoted basic-string TOML key would
  // be a parse error; the renderer collapses runs of CR/LF to a space.
  assert.doesNotMatch(out.files['manifest.toml']!, /name = "[^"]*\n[^"]*"/)
})

test('buildExportedFiles produces an index.ts that compiles to a Plugin shape', () => {
  const out = buildExportedFiles('Focus', SAMPLE_LAYOUT)
  const src = out.files['index.ts']!
  // Pin the load-bearing pieces so a future refactor can't silently
  // regress the emitted contract.
  assert.match(src, /import type \{ Plugin, PluginAPI \} from '\.\.\/\.\.\/\.\.\/types\/plugin'/)
  assert.match(src, /import layoutData from '\.\/focus\.layout\.json'/)
  assert.match(src, /api\.workspace\.applySnapshot\(layout\)/)
  assert.match(src, /api\.commands\.register\('focus\.apply'/)
  assert.match(src, /activationEvents: \['onStartup'\]/)
})

test('buildExportedFiles README cites both install paths', () => {
  const out = buildExportedFiles('Focus', SAMPLE_LAYOUT)
  const readme = out.files['README.md']!
  assert.match(readme, /Layout-only \(works today\)/)
  assert.match(readme, /first-party shell plugin \(developer build\)/)
  assert.match(readme, /WI-44/)
  assert.match(readme, /focus\.layout\.json/)
})

// ── writeExportedPlugin ─────────────────────────────────────────────────────

interface FakeCall {
  pluginId: string
  commandId: string
  args: unknown
}

function makeFakeKernel(): KernelAPI & { calls: FakeCall[] } {
  const calls: FakeCall[] = []
  const k: KernelAPI & { calls: FakeCall[] } = {
    calls,
    async invoke<T>(pluginId: string, commandId: string, args?: unknown): Promise<T> {
      calls.push({ pluginId, commandId, args: args ?? {} })
      return null as T
    },
    async on(): Promise<() => void> {
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return k
}

test('writeExportedPlugin issues create_dir + four write_file calls in order', async () => {
  const k = makeFakeKernel()
  const files = buildExportedFiles('Focus', SAMPLE_LAYOUT)
  const dir = await writeExportedPlugin(k, files)

  assert.equal(dir, exportDirRelpath('focus'))
  assert.equal(dir, '.forge/exports/focus')

  // First call ensures the directory; the next four write the files.
  assert.equal(k.calls[0]!.commandId, 'create_dir')
  assert.deepEqual(k.calls[0]!.args, { relpath: '.forge/exports/focus' })

  const writes = k.calls.slice(1)
  assert.equal(writes.length, 4)
  for (const call of writes) {
    assert.equal(call.pluginId, 'com.nexus.storage')
    assert.equal(call.commandId, 'write_file')
  }

  const writtenPaths = writes.map((c) => (c.args as { path: string }).path).sort()
  assert.deepEqual(writtenPaths, [
    '.forge/exports/focus/README.md',
    '.forge/exports/focus/focus.layout.json',
    '.forge/exports/focus/index.ts',
    '.forge/exports/focus/manifest.toml',
  ])
})

test('writeExportedPlugin write_file payload is UTF-8 byte array', async () => {
  const k = makeFakeKernel()
  const files = buildExportedFiles('Focus', SAMPLE_LAYOUT)
  await writeExportedPlugin(k, files)
  const manifestWrite = k.calls.find(
    (c) => c.commandId === 'write_file' && (c.args as { path: string }).path.endsWith('manifest.toml'),
  )!
  const bytes = (manifestWrite.args as { bytes: number[] }).bytes
  // Round-trip through the same encoder/decoder pair the writer uses.
  const decoded = new TextDecoder('utf-8').decode(new Uint8Array(bytes))
  assert.equal(decoded, files.files['manifest.toml'])
})

test('writeExportedPlugin tolerates create_dir AlreadyExists', async () => {
  const k: KernelAPI & { calls: FakeCall[] } = {
    calls: [],
    async invoke<T>(pluginId: string, commandId: string, args?: unknown): Promise<T> {
      this.calls.push({ pluginId, commandId, args: args ?? {} })
      if (commandId === 'create_dir') {
        throw new Error('AlreadyExists: directory exists')
      }
      return null as T
    },
    async on(): Promise<() => void> {
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  const files = buildExportedFiles('Focus', SAMPLE_LAYOUT)
  const dir = await writeExportedPlugin(k, files)
  // Successful path even when create_dir throws — the four file
  // writes proceed regardless.
  assert.equal(dir, '.forge/exports/focus')
  const writes = k.calls.filter((c) => c.commandId === 'write_file')
  assert.equal(writes.length, 4)
})
