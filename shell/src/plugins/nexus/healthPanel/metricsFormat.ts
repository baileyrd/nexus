// shell/src/plugins/nexus/healthPanel/metricsFormat.ts
//
// BL-093 follow-up — pure formatters for the kernel `metrics_snapshot`
// shape. Lifted into its own module so the panel's render path stays
// uncluttered and the routing matrix can be unit-tested without
// standing up React or a kernel mock.
//
// The wire shape mirrors `crates/nexus-kernel/src/metrics.rs`:
// `MetricsSnapshot` carries flat string-keyed maps for counters
// (`<plugin>::<command>::<status>`) and per-handler latency
// histograms with p50 / p95 / p99 bucket-upper-bound nanoseconds.

/** Mirror of `nexus_kernel::metrics::HistogramSnapshot`. */
export interface HistogramSnapshot {
  count: number
  sum_ns: number
  mean_ns: number
  p50_ns: number
  p95_ns: number
  p99_ns: number
}

/** Mirror of `nexus_kernel::metrics::MetricsSnapshot`. */
export interface MetricsSnapshot {
  ipc_calls_total: Record<string, number>
  ipc_call_duration: Record<string, HistogramSnapshot>
  event_bus_published_total: Record<string, number>
  capability_checks_total: Record<string, number>
  plugin_lifecycle_duration: Record<string, HistogramSnapshot>
  event_bus_queue_depth: number
  metrics_dropped_total: number
}

/** A row in the IPC table that joins a counter row with its matching
 *  duration histogram. The kernel emits both as separate maps keyed
 *  off `<plugin>::<command>` (durations) and
 *  `<plugin>::<command>::<status>` (counters); the join leaves the
 *  ipc_calls_total values denormalised across statuses but keeps the
 *  duration on a single row per (plugin, command) tuple. */
export interface IpcRow {
  plugin: string
  command: string
  total: number
  ok: number
  errors: number
  histogram: HistogramSnapshot | null
}

/** Outcome buckets the metrics writer produces; extras roll into
 *  `errors` defensively so a future status doesn't drop off the chart. */
const OK_STATUSES = new Set(['ok'])

/** Format a nanosecond duration to a short, human-readable string with
 *  one of the four common units (ns / µs / ms / s). Picks the unit
 *  whose magnitude lands the value in the [1, 1000) range when
 *  possible, falling back to seconds for very large numbers.
 *
 *  Returns `'—'` for zero / NaN — distinguishes "no observation" from
 *  "very fast" in the table. */
export function formatDuration(ns: number): string {
  if (!Number.isFinite(ns) || ns <= 0) return '—'
  if (ns < 1_000) return `${ns.toFixed(0)} ns`
  if (ns < 1_000_000) return `${(ns / 1_000).toFixed(1)} µs`
  if (ns < 1_000_000_000) return `${(ns / 1_000_000).toFixed(1)} ms`
  return `${(ns / 1_000_000_000).toFixed(2)} s`
}

/** Format a counter value; returns `'0'` literally rather than `'—'`
 *  so a zeroed row is visibly distinct from a missing one. */
export function formatCount(n: number): string {
  if (!Number.isFinite(n)) return '0'
  return n.toLocaleString('en-US')
}

/** Walk the `<plugin>::<command>::<status>` counter keys + the
 *  `<plugin>::<command>` duration keys and bucket them into joined
 *  rows. A duration entry without a counter (vanishingly rare —
 *  recording the duration always pairs with recording the status) is
 *  surfaced anyway with zero counts so the latency reading isn't lost.
 *
 *  Rows sort by total count descending so the noisiest path is at
 *  the top — the most actionable signal in a triage context. */
export function buildIpcRows(snapshot: MetricsSnapshot): IpcRow[] {
  const rows = new Map<string, IpcRow>()
  for (const [key, value] of Object.entries(snapshot.ipc_calls_total)) {
    const parts = key.split('::')
    if (parts.length < 3) continue
    const status = parts[parts.length - 1]!
    const command = parts[parts.length - 2]!
    const plugin = parts.slice(0, -2).join('::')
    const rowKey = `${plugin}::${command}`
    let row = rows.get(rowKey)
    if (!row) {
      row = {
        plugin,
        command,
        total: 0,
        ok: 0,
        errors: 0,
        histogram: null,
      }
      rows.set(rowKey, row)
    }
    row.total += value
    if (OK_STATUSES.has(status)) {
      row.ok += value
    } else {
      row.errors += value
    }
  }
  for (const [key, hist] of Object.entries(snapshot.ipc_call_duration)) {
    let row = rows.get(key)
    if (!row) {
      const sep = key.indexOf('::')
      const plugin = sep === -1 ? '' : key.slice(0, sep)
      const command = sep === -1 ? key : key.slice(sep + 2)
      row = { plugin, command, total: 0, ok: 0, errors: 0, histogram: null }
      rows.set(key, row)
    }
    row.histogram = hist
  }
  const out = [...rows.values()]
  out.sort((a, b) => {
    if (b.total !== a.total) return b.total - a.total
    if (a.plugin !== b.plugin) return a.plugin.localeCompare(b.plugin)
    return a.command.localeCompare(b.command)
  })
  return out
}

/** Aggregate `event_bus_published_total{plugin}` into a flat array
 *  sorted by count desc; ties broken alphabetically. */
export function buildEventBusRows(
  snapshot: MetricsSnapshot,
): Array<{ plugin: string; total: number }> {
  const rows = Object.entries(snapshot.event_bus_published_total).map(
    ([plugin, total]) => ({ plugin, total }),
  )
  rows.sort((a, b) => {
    if (b.total !== a.total) return b.total - a.total
    return a.plugin.localeCompare(b.plugin)
  })
  return rows
}

/** Aggregate `capability_checks_total{plugin, capability, status}`
 *  rows into per-(plugin, capability) tuples with separate granted /
 *  denied counts so the panel can flag denials prominently. */
export function buildCapabilityRows(
  snapshot: MetricsSnapshot,
): Array<{ plugin: string; capability: string; granted: number; denied: number }> {
  const rows = new Map<
    string,
    { plugin: string; capability: string; granted: number; denied: number }
  >()
  for (const [key, value] of Object.entries(snapshot.capability_checks_total)) {
    const parts = key.split('::')
    if (parts.length < 3) continue
    const status = parts[parts.length - 1]!
    const capability = parts[parts.length - 2]!
    const plugin = parts.slice(0, -2).join('::')
    const rowKey = `${plugin}::${capability}`
    let row = rows.get(rowKey)
    if (!row) {
      row = { plugin, capability, granted: 0, denied: 0 }
      rows.set(rowKey, row)
    }
    if (status === 'granted' || status === 'ok') {
      row.granted += value
    } else {
      row.denied += value
    }
  }
  const out = [...rows.values()]
  out.sort((a, b) => {
    if (b.denied !== a.denied) return b.denied - a.denied
    if (a.plugin !== b.plugin) return a.plugin.localeCompare(b.plugin)
    return a.capability.localeCompare(b.capability)
  })
  return out
}
