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
import { gzipSync } from 'node:zlib'
import { performance } from 'node:perf_hooks'
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

function main(): void {
  const writeMode = process.argv.includes('--write')

  const buildSec = runBuild()
  const buildReport = inspectBuild(buildSec)

  console.error('[perf] running microbenchmarks...')
  const flatten = microbenchFlattenTree()
  const frame = microbenchFrameSnapshot()

  const report: PerfReport = {
    schemaVersion: SCHEMA_VERSION,
    generatedAt: new Date().toISOString(),
    host: host(),
    build: buildReport,
    microbenchmarks: {
      'flattenTree.10k': flatten,
      'frameSnapshot.4stores.flush': frame,
    },
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

main()
