// BL-054 Phase 4 — pure aggregators over ActivityEntry[].
//
// The BL-054 Phase 4 plan calls for a usage panel powered by
// `com.nexus.ai::activity_list`. Today the ActivityEntry schema doesn't
// carry token / cost numbers — that's tracked separately and would
// require a schema bump (deferred). What the schema DOES carry is
// surface, origin, outcome, and timestamp — enough to render a
// per-surface and per-day "what's been happening" rollup.
//
// This module is the pure projection layer: in, ActivityEntry[]; out,
// the aggregates the view needs. Side-effect-free; trivial to unit
// test in isolation from the IPC + bus layers.

import type {
  ActivityEntry,
  ActivityOutcome,
  ActivitySurface,
} from '../activityTimeline/activityTimelineStore'

export interface SurfaceCounts {
  surface: ActivitySurface
  total: number
  ok: number
  error: number
  cancelled: number
}

export interface DailyCounts {
  /** ISO `YYYY-MM-DD`. */
  date: string
  total: number
  ok: number
  error: number
}

export interface UsageRollup {
  /** Total entries across every surface. */
  total: number
  /** Per-surface breakdown, sorted by total desc. */
  bySurface: SurfaceCounts[]
  /** Per-day breakdown for the last `dayWindow` days, oldest-first. */
  byDay: DailyCounts[]
  /** Most recent entry timestamp, or null when entries is empty. */
  latest: string | null
}

/** Roll a list of activity entries up into per-surface + per-day
 *  counts. `dayWindow` controls the byDay range — defaults to 14 to
 *  match the panel's "two-week" rendering. Days with zero activity are
 *  still emitted so the chart doesn't have gaps. */
export function aggregateUsage(
  entries: readonly ActivityEntry[],
  dayWindow = 14,
  now: Date = new Date(),
): UsageRollup {
  const bySurfaceMap = new Map<ActivitySurface, SurfaceCounts>()
  const byDayMap = new Map<string, DailyCounts>()
  let latest: string | null = null

  for (const entry of entries) {
    const surfaceCounts = bySurfaceMap.get(entry.surface)
      ?? { surface: entry.surface, total: 0, ok: 0, error: 0, cancelled: 0 }
    surfaceCounts.total += 1
    bumpOutcome(surfaceCounts, entry.outcome)
    bySurfaceMap.set(entry.surface, surfaceCounts)

    const date = entry.timestamp.slice(0, 10) // ISO date prefix
    if (date && /^\d{4}-\d{2}-\d{2}$/.test(date)) {
      const day = byDayMap.get(date) ?? { date, total: 0, ok: 0, error: 0 }
      day.total += 1
      if (entry.outcome === 'ok') day.ok += 1
      else if (entry.outcome === 'error') day.error += 1
      byDayMap.set(date, day)
    }

    if (latest === null || entry.timestamp > latest) latest = entry.timestamp
  }

  const bySurface = Array.from(bySurfaceMap.values()).sort(
    (a, b) => b.total - a.total || a.surface.localeCompare(b.surface),
  )

  const byDay = backfillDayWindow(byDayMap, dayWindow, now)

  return { total: entries.length, bySurface, byDay, latest }
}

function bumpOutcome(counts: SurfaceCounts, outcome: ActivityOutcome): void {
  switch (outcome) {
    case 'ok': counts.ok += 1; break
    case 'error': counts.error += 1; break
    case 'cancelled': counts.cancelled += 1; break
  }
}

/** Produce a contiguous window of `dayWindow` ISO dates ending on
 *  `now` (inclusive) and merge the observed counts into it. Missing
 *  days get zeroed entries so the byDay timeline has no gaps. */
function backfillDayWindow(
  byDayMap: Map<string, DailyCounts>,
  dayWindow: number,
  now: Date,
): DailyCounts[] {
  if (dayWindow <= 0) return []
  const out: DailyCounts[] = []
  for (let offset = dayWindow - 1; offset >= 0; offset--) {
    const d = new Date(now)
    d.setUTCDate(d.getUTCDate() - offset)
    const date = d.toISOString().slice(0, 10)
    out.push(byDayMap.get(date) ?? { date, total: 0, ok: 0, error: 0 })
  }
  return out
}
