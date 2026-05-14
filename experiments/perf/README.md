# Nexus shell perf harness (BL-112)

Repo-committed numeric snapshots so per-PR perf regressions show up as
diffs against committed baselines, not as user reports.

## What's measured

Two classes of metric:

- **Build-output, deterministic.** `dist/` chunk inventory after a
  clean `pnpm --filter nexus-shell build`: total eager-vs-lazy bytes,
  entry chunk's static-import set, every `<link rel="modulepreload">`
  tag in `index.html`. These are byte-stable across runs on the same
  source — a diff in the committed baseline is a real regression.
  This class would have caught the BL-111 vendor-mermaid eager-preload
  bug at PR review.
- **Microbenchmarks, noisy.** `flattenTree` walks a synthetic 10k-file
  forge with every folder expanded; `frameSnapshot` flushes one frame
  of mutations across four stores. Median (p50) µs per iteration is
  the comparable number; p95/p99 captures tail latency. Wall-clock
  varies ±20% on a contended host — treat as a smoke gate, not a
  precise budget.
- **Editor typing-latency scenarios (BL-122).** Three families backing
  the [TYPING-LATENCY-PLAN](../../docs/roadmap/TYPING-LATENCY-PLAN.md):
  - `editor.apply_transaction.{small,medium,large}` — drives
    `EditorCorePlugin::dispatch(HANDLER_APPLY_TRANSACTION, ...)` against
    10 / 100 / 5000-block docs via the Rust integration test
    `crates/nexus-editor/tests/perf_apply_transaction.rs`. The current
    baseline shows the N-linear shape of the snapshot-serialize path —
    BL-123 lands the slim text-only response that flattens the curve.
  - `editor.livePreview.decorate.{small,medium,large}` — calls
    `buildLivePreviewDecorations` over a fully-parsed lezer tree at
    50 / 500 / 1500 lines. Forces a complete parse via
    `ensureSyntaxTree` so doc length actually drives node count
    (without it lezer's incremental parser caps the tree). BL-125
    moves the walker to visible-viewport scope; the win shows up as
    `large` collapsing toward `small`.
  - `editor.markdownRender.large` — `renderMarkdown` (marked +
    DOMPurify) on a 1500-line synthetic doc. Tracks the long-doc
    preview render path that fires on hydration / tab-switch.

The wall-clock build duration is recorded for context but is the
noisiest metric in the file; don't gate on it.

## How to run

From `shell/` so Node resolves `tsx` and the shell-hoisted CM6 /
happy-dom packages:

```sh
cd shell
node --import tsx --max-old-space-size=8192 ../experiments/perf/run.ts          # print to stdout
node --import tsx --max-old-space-size=8192 ../experiments/perf/run.ts --write  # also write baselines/<YYYY-MM-DD>.json
```

The harness depends on the shell workspace's `tsx`, plus
`@happy-dom/global-registrator` and CodeMirror packages reached via a
`createRequire` rooted at `shell/package.json`. No separate install
needed.

The raised heap (`--max-old-space-size=8192`) accommodates the
lezer-markdown syntax tree + marked/DOMPurify allocations for the
`large` editor scenarios; Node's default 4 GB cap OOMs around the
1500-line markdown render.

Run takes ~2–3 min end-to-end (Vite build + a release-mode
`cargo test` for the kernel-side editor scenarios).

## Host-machine assumption

Microbenchmark numbers are anchored to whatever machine runs them.
The committed baselines under `baselines/` were captured on:

- WSL2 on Windows 11
- Linux 6.6 / Node v22+
- AMD Ryzen-class CPU
- Idle but not sealed (browser, editor, etc. running)

A different host will produce different absolute numbers; the
comparison surface is **same baseline, before vs after a change** on
the **same machine**, not cross-machine absolutes.

## Output schema

`PerfReport` shape — see `run.ts` for the TypeScript definitions.
Top-level keys:

- `schemaVersion: 1` — bump when the JSON shape changes; older
  baselines must be re-captured rather than diffed against.
- `generatedAt` — ISO timestamp.
- `host` — platform / arch / Node / CPU model + count / total RAM.
- `build` — chunk inventory, eager vs lazy totals, entry's static
  imports, HTML preloads.
- `microbenchmarks` — per-bench p50/p95/p99/mean µs + iteration count.

The output is sorted (chunks by size desc, static imports
alphabetically, etc.) so re-runs produce textually-identical baselines
on the same source.

## Roadmap

The BL-112 DoD calls for ≥5 scenarios under WebDriver/Tauri (cold
start trace, "open 50 MB file", "scroll a 10k-row file tree", "render
500-heading outline", "type 5 chars in 5k-line markdown"). Those
need a stable WDIO-Tauri runner and aren't in this first cut. They
slot into this harness as additional `scenarios` entries on the
`PerfReport` object once the runner exists. Until then, this file
covers the static-analysis class — which is what would have caught
BL-111.

A regression-gate CI job that fails on any deterministic-metric diff
+ >20% drift on noisy metrics is the next layer; deferred per the BL
itself.
