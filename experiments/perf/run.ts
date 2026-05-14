/**
 * Nexus shell perf harness (BL-112).
 *
 * Produces a stable JSON snapshot of build-output and microbenchmark
 * metrics so per-PR regressions are visible in `git diff` against
 * the committed baselines under `experiments/perf/baselines/`.
 *
 * Run from the repo root:
 *
 *     node --import tsx experiments/perf/run.ts
 *
 * Append `--write` to also write `experiments/perf/baselines/<date>.json`.
 *
 * The Tauri / WDIO scenarios named in the BL-112 DoD (cold-start trace,
 * "open 50 MB file", "scroll 10k-row tree", "render 500-heading
 * outline", "type 5 chars in 5k-line markdown") still need a runner;
 * they slot into this harness as additional `scenarios` entries once
 * the WDIO setup lands. See README.md §Roadmap.
 */

import { execSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { gzipSync } from 'node:zlib'
import { performance } from 'node:perf_hooks'
import { pathToFileURL } from 'node:url'
import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'

import { flattenTree } from '../../shell/src/plugins/nexus/files/flattenTree.ts'
import type { FilesDirEntry } from '../../shell/src/plugins/nexus/files/filesStore.ts'
import {
  FrameSnapshot,
  snap,
  type Subscribable,
} from '../../shell/src/stores/frameSnapshot.ts'

// BL-122 typing-latency scaffold: live-preview decoration + markdown
// render scenarios need a DOM (DOMPurify, widget classes that reference
// document) plus CodeMirror, all hoisted under `shell/node_modules/`
// rather than the repo root. Resolve those bare specifiers through a
// `createRequire` rooted in shell so the harness keeps running from
// the repo-root invocation documented in `experiments/perf/README.md`.
let domReady = false
const SHELL_REQUIRE = createRequire(path.join(/* SHELL_DIR set below */ __dirname, '..', '..', 'shell', 'package.json'))
async function importFromShell<T>(spec: string): Promise<T> {
  const resolved = SHELL_REQUIRE.resolve(spec)
  return (await import(pathToFileURL(resolved).href)) as T
}
async function ensureDom(): Promise<void> {
  if (domReady) return
  const { GlobalRegistrator } = await importFromShell<{
    GlobalRegistrator: { register(): void }
  }>('@happy-dom/global-registrator')
  GlobalRegistrator.register()
  domReady = true
}

const REPO_ROOT = path.resolve(__dirname, '..', '..')
const SHELL_DIR = path.join(REPO_ROOT, 'shell')
const DIST_DIR = path.join(SHELL_DIR, 'dist')
const ASSETS_DIR = path.join(DIST_DIR, 'assets')
const BASELINES_DIR = path.join(__dirname, 'baselines')

const SCHEMA_VERSION = 1

interface PerfReport {
  schemaVersion: number
  generatedAt: string
  host: HostInfo
  build: BuildReport
  microbenchmarks: Record<string, MicrobenchResult>
}

interface HostInfo {
  platform: NodeJS.Platform
  arch: string
  nodeVersion: string
  cpuModel: string
  cpuCount: number
  totalMemMB: number
}

interface BuildReport {
  /** Wall-clock seconds for `pnpm --filter nexus-shell build` from
   *  a clean `dist/`. Noisy — varies with disk + load. */
  durationSec: number
  entryChunk: { name: string; sizeKB: number; sizeKBGz: number; staticImports: string[] }
  htmlPreloads: string[]
  /** Sum of every chunk the entry HTML preloads or static-imports
   *  through the entry chunk. The cost the user pays before clicking
   *  anything. */
  eagerKB: number
  eagerKBGz: number
  /** Sum of every chunk reachable only through dynamic imports. */
  lazyKB: number
  lazyKBGz: number
  /** Per-chunk inventory, sorted by raw size desc. */
  chunks: ChunkSummary[]
}

interface ChunkSummary {
  name: string
  sizeKB: number
  sizeKBGz: number
  /** True iff the chunk is preloaded by `index.html` or
   *  static-imported by the entry chunk (transitively). */
  eager: boolean
}

interface MicrobenchResult {
  iterations: number
  /** Microseconds per iteration. */
  p50us: number
  p95us: number
  p99us: number
  meanus: number
  totalMs: number
}

function host(): HostInfo {
  const cpus = os.cpus()
  return {
    platform: process.platform,
    arch: process.arch,
    nodeVersion: process.version,
    cpuModel: cpus[0]?.model.trim() ?? 'unknown',
    cpuCount: cpus.length,
    totalMemMB: Math.round(os.totalmem() / 1024 / 1024),
  }
}

function runBuild(): number {
  console.error('[perf] cleaning dist/ + running pnpm build...')
  fs.rmSync(DIST_DIR, { recursive: true, force: true })
  const t0 = performance.now()
  execSync('pnpm --filter nexus-shell build', { cwd: REPO_ROOT, stdio: 'pipe' })
  return (performance.now() - t0) / 1000
}

function inspectBuild(durationSec: number): BuildReport {
  const html = fs.readFileSync(path.join(DIST_DIR, 'index.html'), 'utf-8')

  const htmlPreloads = Array.from(
    html.matchAll(/<link rel="modulepreload"[^>]*href="\/assets\/([^"]+)"/g),
  ).map((m) => m[1])

  const entryName = (html.match(/<script type="module"[^>]*src="\/assets\/([^"]+)"/) ?? [])[1]
  if (!entryName) throw new Error('Could not locate entry script in index.html')

  const entrySrc = fs.readFileSync(path.join(ASSETS_DIR, entryName), 'utf-8')
  const entryStaticImports = Array.from(entrySrc.matchAll(/from"\.\/([^"]+\.js)"/g))
    .map((m) => m[1])
    .sort()
    .filter((name, i, arr) => arr.indexOf(name) === i)

  const entryRaw = fs.statSync(path.join(ASSETS_DIR, entryName)).size
  const entryGz = gzipSync(entrySrc).byteLength

  // Eager set: the entry chunk + everything it static-imports + every
  // HTML modulepreload. (HTML preloads can include chunks the entry
  // doesn't directly import, e.g. the polyfill helper.)
  const eagerNames = new Set<string>([entryName, ...entryStaticImports, ...htmlPreloads])

  const chunks: ChunkSummary[] = []
  let eagerKB = 0
  let eagerKBGz = 0
  let lazyKB = 0
  let lazyKBGz = 0

  for (const name of fs.readdirSync(ASSETS_DIR)) {
    if (!name.endsWith('.js')) continue
    const filePath = path.join(ASSETS_DIR, name)
    const raw = fs.statSync(filePath).size
    const gz = gzipSync(fs.readFileSync(filePath)).byteLength
    const eager = eagerNames.has(name)
    chunks.push({
      name,
      sizeKB: round1(raw / 1024),
      sizeKBGz: round1(gz / 1024),
      eager,
    })
    if (eager) {
      eagerKB += raw
      eagerKBGz += gz
    } else {
      lazyKB += raw
      lazyKBGz += gz
    }
  }
  chunks.sort((a, b) => b.sizeKB - a.sizeKB)

  return {
    durationSec: round2(durationSec),
    entryChunk: {
      name: entryName,
      sizeKB: round1(entryRaw / 1024),
      sizeKBGz: round1(entryGz / 1024),
      staticImports: entryStaticImports,
    },
    htmlPreloads,
    eagerKB: round1(eagerKB / 1024),
    eagerKBGz: round1(eagerKBGz / 1024),
    lazyKB: round1(lazyKB / 1024),
    lazyKBGz: round1(lazyKBGz / 1024),
    chunks,
  }
}

function round1(n: number): number {
  return Math.round(n * 10) / 10
}
function round2(n: number): number {
  return Math.round(n * 100) / 100
}

function timeBlock(fn: () => void, iterations: number): MicrobenchResult {
  const samples: number[] = []
  // Warmup
  for (let i = 0; i < Math.min(50, iterations); i++) fn()
  const t0 = performance.now()
  for (let i = 0; i < iterations; i++) {
    const a = performance.now()
    fn()
    samples.push((performance.now() - a) * 1000) // µs
  }
  const totalMs = performance.now() - t0
  samples.sort((a, b) => a - b)
  return {
    iterations,
    p50us: round2(percentile(samples, 0.5)),
    p95us: round2(percentile(samples, 0.95)),
    p99us: round2(percentile(samples, 0.99)),
    meanus: round2(samples.reduce((s, v) => s + v, 0) / samples.length),
    totalMs: round2(totalMs),
  }
}

function percentile(sortedAsc: number[], p: number): number {
  if (sortedAsc.length === 0) return 0
  const idx = Math.min(sortedAsc.length - 1, Math.max(0, Math.floor(sortedAsc.length * p)))
  return sortedAsc[idx]
}

function buildSyntheticForge(fileCount: number, branchingFactor = 10): {
  root: FilesDirEntry[]
  children: Record<string, FilesDirEntry[]>
  expanded: Set<string>
} {
  // Build a tree with `fileCount` files distributed across folders of
  // depth `log_branching(fileCount)`. Every folder is expanded so the
  // walker has to descend the full tree.
  const children: Record<string, FilesDirEntry[]> = {}
  const expanded = new Set<string>()
  const root: FilesDirEntry[] = []
  let nextId = 0

  function makeBranch(parent: string, depth: number, remaining: number): FilesDirEntry[] {
    const entries: FilesDirEntry[] = []
    if (depth === 0 || remaining <= branchingFactor) {
      for (let i = 0; i < remaining; i++) {
        const name = `file-${nextId++}.md`
        entries.push({ name, relpath: parent ? `${parent}/${name}` : name, isDir: false })
      }
      return entries
    }
    const perChild = Math.ceil(remaining / branchingFactor)
    let placed = 0
    for (let i = 0; i < branchingFactor && placed < remaining; i++) {
      const folderName = `dir-${nextId++}`
      const folderRel = parent ? `${parent}/${folderName}` : folderName
      const dir: FilesDirEntry = { name: folderName, relpath: folderRel, isDir: true }
      entries.push(dir)
      const allotted = Math.min(perChild, remaining - placed)
      const kids = makeBranch(folderRel, depth - 1, allotted)
      children[folderRel] = kids
      expanded.add(folderRel)
      placed += allotted
    }
    return entries
  }

  const depth = Math.ceil(Math.log(fileCount) / Math.log(branchingFactor))
  const top = makeBranch('', depth, fileCount)
  root.push(...top)
  return { root, children, expanded }
}

function microbenchFlattenTree(): MicrobenchResult {
  const { root, children, expanded } = buildSyntheticForge(10000)
  return timeBlock(() => {
    flattenTree(root, children, expanded, 'nameAsc')
  }, 200)
}

function microbenchFrameSnapshot(): MicrobenchResult {
  // Synthetic Subscribable that mimics zustand's surface without a
  // real zustand dep — frameSnapshot only consumes `getState` +
  // `subscribe`.
  const makeStore = (): Subscribable<{ n: number }> & { mutate: () => void } => {
    let state = { n: 0 }
    const subs = new Set<() => void>()
    return {
      getState: () => state,
      subscribe: (cb) => {
        subs.add(cb)
        return () => subs.delete(cb)
      },
      mutate: () => {
        state = { n: state.n + 1 }
        subs.forEach((cb) => cb())
      },
    }
  }
  const a = makeStore()
  const b = makeStore()
  const c = makeStore()
  const d = makeStore()
  // Manual scheduler so flush is deterministic.
  let pending: (() => void) | null = null
  const fs = new FrameSnapshot(
    [
      snap(a, (s) => s.n),
      snap(b, (s) => s.n),
      snap(c, (s) => s.n),
      snap(d, (s) => s.n),
    ],
    (cb) => {
      pending = cb
      return () => {
        if (pending === cb) pending = null
      }
    },
  )
  fs.start()
  return timeBlock(() => {
    // Mutate every store, then flush — measures one "frame" of
    // multi-store updates.
    a.mutate()
    b.mutate()
    c.mutate()
    d.mutate()
    pending?.()
    pending = null
  }, 5000)
}

/**
 * BL-122 — `editor.apply_transaction.{small,medium,large}` scenarios.
 *
 * The Rust integration test at
 * `crates/nexus-editor/tests/perf_apply_transaction.rs` does the
 * actual timing (release-mode, no Tauri/IPC). It prints one stable
 * `PERF_RESULT::<json>` line per scenario when `NEXUS_PERF=1` is set.
 * We invoke it via `cargo test --release` here and parse those lines
 * back into the per-scenario `MicrobenchResult` shape.
 */
function microbenchKernelApplyTransaction(): Record<string, MicrobenchResult> {
  console.error('[perf] running editor.apply_transaction.* (cargo test, release)...')
  const out = execSync(
    'cargo test -p nexus-editor --release --test perf_apply_transaction -- --nocapture',
    {
      cwd: REPO_ROOT,
      env: { ...process.env, NEXUS_PERF: '1' },
      stdio: ['ignore', 'pipe', 'inherit'],
      encoding: 'utf8',
    },
  )
  const results: Record<string, MicrobenchResult> = {}
  for (const line of out.split('\n')) {
    const marker = 'PERF_RESULT::'
    const idx = line.indexOf(marker)
    if (idx < 0) continue
    const parsed = JSON.parse(line.slice(idx + marker.length)) as {
      name: string
      iterations: number
      meanus: number
      p50us: number
      p95us: number
      p99us: number
      totalMs: number
    }
    results[parsed.name] = {
      iterations: parsed.iterations,
      p50us: parsed.p50us,
      p95us: parsed.p95us,
      p99us: parsed.p99us,
      meanus: parsed.meanus,
      totalMs: parsed.totalMs,
    }
  }
  if (Object.keys(results).length !== 3) {
    throw new Error(
      `expected 3 editor.apply_transaction.* results, got ${Object.keys(results).length}`,
    )
  }
  return results
}

interface DocSize {
  name: string
  lineCount: number
  iterations: number
}

const LIVE_PREVIEW_SIZES: DocSize[] = [
  { name: 'small', lineCount: 50, iterations: 100 },
  { name: 'medium', lineCount: 500, iterations: 40 },
  // BL-125: with viewport-scoped inline decorations the per-frame
  // walk is bounded by viewport size, not doc size, so a much larger
  // doc is safe under the same iteration budget. 5000 lines exercises
  // the win — the inline walker should stay flat with `small` while
  // the block walker absorbs the full-tree pass once.
  { name: 'large', lineCount: 5000, iterations: 20 },
]

/**
 * Synthetic viewport range — emulates CM6's `view.visibleRanges`. A
 * realistic viewport on a typing host is ~50 lines tall regardless of
 * doc length, so we keep the range fixed at 50 lines and slide it
 * into the middle of the doc to exercise a non-trivial offset.
 */
function syntheticViewport(doc: string, viewportLines = 50): { from: number; to: number } {
  const lines = doc.split('\n')
  if (lines.length <= viewportLines) {
    return { from: 0, to: doc.length }
  }
  const startLine = Math.max(0, Math.floor((lines.length - viewportLines) / 2))
  let from = 0
  for (let i = 0; i < startLine; i++) {
    from += lines[i]!.length + 1 // +1 for the joining '\n'
  }
  let to = from
  for (let i = 0; i < viewportLines; i++) {
    to += lines[startLine + i]!.length + 1
  }
  return { from, to: Math.min(to, doc.length) }
}

function buildMarkdownDoc(lineCount: number): string {
  // Mix of constructs the live-preview walker handles so the result
  // reflects realistic decoration work: headings, emphasis, code,
  // links, lists, blockquotes. Lines repeat in a stable cycle so two
  // runs produce deterministic input.
  const patterns = [
    'Paragraph with **strong** and *italic* and `inline-code` and [link](http://example.com).',
    '## Heading L2 with `code` mix',
    '- bullet list item with *emphasis*',
    '> blockquote line with **bold**',
    '',
    'Plain paragraph number {i} sits between heavier constructs.',
  ]
  const lines: string[] = []
  for (let i = 0; i < lineCount; i++) {
    const p = patterns[i % patterns.length]!
    lines.push(p.replace('{i}', String(i)))
  }
  return lines.join('\n')
}

/**
 * BL-122 / BL-125 — `editor.livePreview.decorate.{small,medium,large}` scenarios.
 *
 * Each iteration emulates one frame of CM6 work: one `block`
 * decoration rebuild (full-tree walk, StateField source) + one
 * `inline` decoration rebuild scoped to a synthetic 50-line viewport
 * (ViewPlugin source). Pre-BL-125 the inline walk traversed the full
 * syntax tree on every doc/selection change; BL-125 makes it
 * viewport-bounded, so the `large` scenario should stay roughly flat
 * with `small` for the inline portion while the block portion
 * absorbs the full-tree pass (which is cheap because block
 * constructs are rare).
 */
/**
 * BL-127 Phase A — `editor.typing.{small,medium,large}` scenarios.
 *
 * Mounts a real CodeMirror EditorView under happy-dom with the
 * production `livePreviewExt` (BL-125's StateField + ViewPlugin
 * split) + the markdown language extension. Dispatches N
 * keystroke-shaped transactions and measures each dispatch's wall
 * time. The number reflects the CM6 → syntax-tree → StateField /
 * ViewPlugin recompute → DOM render path on a happy-dom layout
 * engine — which is what BL-123 / BL-124 / BL-125 / BL-126
 * collectively optimise on the editor side.
 *
 * **Not measured**: Tauri IPC serialisation, real GPU paint, React
 * re-render through `EditorView.tsx`'s prop pipeline. Those need a
 * WDIO-Tauri runner that doesn't exist yet — same gating BL-112's
 * runtime scenarios called out. The Phase A baseline + the
 * `VITE_NEXUS_PERF_TYPING=1` production hook (see EditorView.tsx)
 * cover the editor-engine half today.
 */
async function microbenchEditorTyping(): Promise<Record<string, MicrobenchResult>> {
  await ensureDom()
  const { EditorState } = await importFromShell<typeof import('@codemirror/state')>(
    '@codemirror/state',
  )
  const { EditorView } = await importFromShell<typeof import('@codemirror/view')>(
    '@codemirror/view',
  )
  const { markdown } = await importFromShell<typeof import('@codemirror/lang-markdown')>(
    '@codemirror/lang-markdown',
  )
  const { ensureSyntaxTree } = await importFromShell<typeof import('@codemirror/language')>(
    '@codemirror/language',
  )
  const { livePreviewExt } = await import(
    '../../shell/src/plugins/nexus/editor/cm/livePreview.ts'
  )
  const results: Record<string, MicrobenchResult> = {}
  // Re-use the same size buckets as live-preview so a per-keystroke
  // regression in BL-123 / BL-125 shows up here at the matching
  // scale.
  for (const sz of LIVE_PREVIEW_SIZES) {
    const doc = buildMarkdownDoc(sz.lineCount)
    const parent = document.createElement('div')
    document.body.appendChild(parent)
    const view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [markdown(), livePreviewExt()],
      }),
      parent,
    })
    // Force a complete lezer parse before the first measured
    // dispatch — same reason as the decoration scenario.
    ensureSyntaxTree(view.state, view.state.doc.length, 30_000)
    let insertPos = view.state.doc.length
    try {
      results[`editor.typing.${sz.name}`] = timeBlock(() => {
        view.dispatch({
          changes: { from: insertPos, to: insertPos, insert: 'x' },
        })
        insertPos += 1
      }, sz.iterations)
    } finally {
      view.destroy()
      parent.remove()
    }
  }
  return results
}

async function microbenchLivePreviewDecorate(): Promise<Record<string, MicrobenchResult>> {
  await ensureDom()
  const { EditorState } = await importFromShell<typeof import('@codemirror/state')>(
    '@codemirror/state',
  )
  const { markdown } = await importFromShell<typeof import('@codemirror/lang-markdown')>(
    '@codemirror/lang-markdown',
  )
  const { ensureSyntaxTree } = await importFromShell<typeof import('@codemirror/language')>(
    '@codemirror/language',
  )
  const {
    buildLivePreviewBlockDecorations,
    buildLivePreviewInlineDecorations,
  } = await import(
    '../../shell/src/plugins/nexus/editor/cm/livePreviewDecorations.ts'
  )
  const results: Record<string, MicrobenchResult> = {}
  for (const sz of LIVE_PREVIEW_SIZES) {
    const doc = buildMarkdownDoc(sz.lineCount)
    const state = EditorState.create({ doc, extensions: [markdown()] })
    // Force a complete parse before timing the decoration walk. lezer
    // parses incrementally; without this the first `syntaxTree(state)`
    // call only walks whatever the time-budgeted parse finished, and
    // every doc size produces ~the same number — useless as a baseline
    // for BL-125's viewport-scoping win.
    ensureSyntaxTree(state, doc.length, 30_000)
    const viewport = syntheticViewport(doc)
    results[`editor.livePreview.decorate.${sz.name}`] = timeBlock(() => {
      buildLivePreviewBlockDecorations(state)
      buildLivePreviewInlineDecorations(state, [viewport])
    }, sz.iterations)
  }
  return results
}

/**
 * BL-122 — `editor.markdownRender.large` scenario.
 *
 * Runs the shell-side `renderMarkdown` (marked + DOMPurify) on a 5k-
 * line doc. Single size by design — the kernel-side scenarios cover
 * the small/medium spectrum; this one measures the long-doc preview-
 * render path that fires on tab-switch / hydration.
 */
async function microbenchMarkdownRenderLarge(): Promise<MicrobenchResult> {
  await ensureDom()
  const { renderMarkdown } = await import(
    '../../shell/src/plugins/nexus/editor/markdownRender.ts'
  )
  // 1500 lines mirrors the live-preview `large` ceiling — DOMPurify
  // and marked together hold enough intermediate state that a 5k-
  // line doc OOMs the harness on a 4 GB Node heap.
  const doc = buildMarkdownDoc(1500)
  return timeBlock(() => {
    renderMarkdown(doc)
  }, 10)
}

async function main(): Promise<void> {
  const writeMode = process.argv.includes('--write')

  const buildSec = runBuild()
  const buildReport = inspectBuild(buildSec)

  console.error('[perf] running microbenchmarks...')
  const flatten = microbenchFlattenTree()
  const frame = microbenchFrameSnapshot()
  const apply = microbenchKernelApplyTransaction()
  console.error('[perf] running editor.livePreview.decorate.* (happy-dom + CM6)...')
  const livePreview = await microbenchLivePreviewDecorate()
  console.error('[perf] running editor.typing.* (BL-127 — mounted CM6 keystrokes)...')
  const typing = await microbenchEditorTyping()
  console.error('[perf] running editor.markdownRender.large (marked + DOMPurify)...')
  const markdownLarge = await microbenchMarkdownRenderLarge()

  // Stable-sorted insertion so the JSON diff stays minimal across
  // runs. The scenarios are grouped: structural helpers first
  // (BL-109/110), then editor hot path (BL-122..127), each group's
  // entries sorted alphabetically.
  const microbenchmarks: Record<string, MicrobenchResult> = {
    'flattenTree.10k': flatten,
    'frameSnapshot.4stores.flush': frame,
    ...sortRecord(apply),
    ...sortRecord(livePreview),
    ...sortRecord(typing),
    'editor.markdownRender.large': markdownLarge,
  }

  const report: PerfReport = {
    schemaVersion: SCHEMA_VERSION,
    generatedAt: new Date().toISOString(),
    host: host(),
    build: buildReport,
    microbenchmarks,
  }

  const json = JSON.stringify(report, null, 2)
  process.stdout.write(json + '\n')

  if (writeMode) {
    const date = new Date().toISOString().slice(0, 10)
    const outPath = path.join(BASELINES_DIR, `${date}.json`)
    fs.mkdirSync(BASELINES_DIR, { recursive: true })
    fs.writeFileSync(outPath, json + '\n')
    console.error(`[perf] wrote ${path.relative(REPO_ROOT, outPath)}`)
  }
}

function sortRecord<T>(rec: Record<string, T>): Record<string, T> {
  const out: Record<string, T> = {}
  for (const k of Object.keys(rec).sort()) out[k] = rec[k]!
  return out
}

void main()
