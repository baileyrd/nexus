import { useMemo, useSyncExternalStore } from 'react'
import {
  FrameSnapshot,
  type SnapshotEntry,
  type SnapshotResult,
} from './frameSnapshot'

/**
 * BL-110 — read N independent stores in lockstep, with one render
 * per animation frame regardless of how many underlying mutations
 * land in between.
 *
 * Usage:
 *
 *     const [tabs, active, count, loading] = useFrameSnapshot([
 *       snap(useEditorStore,    (s) => s.tabs),
 *       snap(useEditorStore,    (s) => s.activeRelpath),
 *       snap(useBacklinksStore, (s) => s.links.length),
 *       snap(useBacklinksStore, (s) => s.loading),
 *     ])
 *
 * The `entries` argument is captured on first render — selectors must
 * be stable for the lifetime of the component (the standard React
 * rule). Pair with `useMemo`/module-level constants if a selector
 * needs to depend on props.
 *
 * BL-124: pass an optional `rebuildKey` when selectors close over a
 * prop that can change for the same component instance (the
 * `relpath` in `EditorView` is the motivating case — each leaf can
 * be re-bound to a different file without unmounting). Changing the
 * key tears down the current FrameSnapshot and stamps a fresh one
 * over the new entries. Omit the key for prop-free entries — the
 * default (`undefined`) is stable.
 *
 * The returned tuple keeps the same array reference within a single
 * rAF window even if a store mutates partway through. Identity flips
 * only when at least one selector value changes between flushes,
 * matching `useSyncExternalStore`'s shallow-equality contract.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function useFrameSnapshot<T extends readonly SnapshotEntry<any, any>[]>(
  entries: T,
  rebuildKey?: unknown,
): SnapshotResult<T> {
  // One controller per `rebuildKey`. With the default `undefined` key,
  // the entries are captured on first render and never rebuilt —
  // preserves BL-110 behaviour. With a non-default key, a key change
  // remounts the controller against the latest entries.
  const snap = useMemo(
    () => new FrameSnapshot(entries),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rebuildKey],
  )

  return useSyncExternalStore(
    (cb) => {
      const dispose = snap.start()
      const unsub = snap.subscribe(cb)
      return () => {
        unsub()
        dispose()
      }
    },
    () => snap.current(),
    () => snap.current(),
  )
}

export { snap } from './frameSnapshot'
