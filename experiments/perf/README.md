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

The wall-clock build duration is recorded for context but is the
noisiest metric in the file; don't gate on it.

## How to run

From the repo root:

```sh
node --import tsx experiments/perf/run.ts            # print to stdout
node --import tsx experiments/perf/run.ts --write    # also write baselines/<YYYY-MM-DD>.json
```

The harness depends on the shell workspace's `tsx`. No separate
install needed; the script imports `flattenTree` and `FrameSnapshot`
straight out of `shell/src/`.

Run takes ~30–60 s end-to-end (most of it is the Vite build).

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
