/**
 * Per-frame multi-store snapshot controller (BL-110).
 *
 * Wraps N independent zustand-style stores so that mutations across
 * them collapse into a single notification per animation frame. The
 * controller is React-free so it can be unit-tested without a DOM —
 * the `useFrameSnapshot` hook in `./useFrameSnapshot.ts` is a thin
 * `useSyncExternalStore` adapter on top of it.
 *
 * Identity guarantee: within one rAF window, repeated calls to
 * `current()` return the same array reference even if underlying
 * stores mutate. The reference only changes after `flush()` detects
 * a per-element diff against the previous values.
 */

/** Minimum surface a store must expose to participate. Matches
 *  zustand's `StoreApi` (also Jotai's `atomStore`, MobX `observable`
 *  via a small adapter, etc.). */
export interface Subscribable<S> {
  getState: () => S
  subscribe: (cb: () => void) => () => void
}

export type SnapshotEntry<S, V> = readonly [Subscribable<S>, (state: S) => V]

/** Sugar for building a typed entry without `as const` boilerplate. */
export function snap<S, V>(store: Subscribable<S>, selector: (state: S) => V): SnapshotEntry<S, V> {
  return [store, selector]
}

/** Inferred tuple of selector return types.
 *
 * `any` on the entry generics is load-bearing here: a heterogeneous
 * tuple of `SnapshotEntry<EditorState, …> | SnapshotEntry<BacklinksState, …>`
 * fails the assignability check against `SnapshotEntry<unknown, …>[]`
 * because the selector parameter is contravariant. `any` opts out of
 * the variance check at the API boundary; per-element typing is
 * recovered via the `infer V` below.
 */
/* eslint-disable @typescript-eslint/no-explicit-any */
export type SnapshotResult<T extends readonly SnapshotEntry<any, any>[]> = {
  -readonly [K in keyof T]: T[K] extends SnapshotEntry<any, infer V> ? V : never
}
/* eslint-enable @typescript-eslint/no-explicit-any */

/** Scheduler abstraction so tests can swap rAF for a synchronous
 *  fake. Returns a `cancel` thunk. */
export type Scheduler = (cb: () => void) => () => void

export const rafScheduler: Scheduler = (cb) => {
  if (typeof requestAnimationFrame === 'undefined') {
    const id = setTimeout(cb, 0)
    return () => clearTimeout(id)
  }
  const id = requestAnimationFrame(cb)
  return () => cancelAnimationFrame(id)
}

// Same variance-escape reason as `SnapshotResult` above — a heterogeneous
// tuple of `SnapshotEntry<StateA, …> | SnapshotEntry<StateB, …>` won't
// assign to a single `SnapshotEntry<X, Y>[]` because the selector
// parameter is contravariant. Per-element typing is preserved via
// `SnapshotResult<T>` on the values field.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export class FrameSnapshot<T extends readonly SnapshotEntry<any, any>[]> {
  private values: SnapshotResult<T>
  private pending = false
  private cancelScheduled: (() => void) | null = null
  private listeners = new Set<() => void>()
  private storeUnsubs: Array<() => void> | null = null

  constructor(
    private entries: T,
    private schedule: Scheduler = rafScheduler,
  ) {
    this.values = this.read()
  }

  /** Read the current values directly from each store. Bypasses the
   *  per-frame cache — useful for the initial snapshot and for flush. */
  private read(): SnapshotResult<T> {
    return this.entries.map(([store, selector]) => selector(store.getState())) as SnapshotResult<T>
  }

  /** The cached tuple. Identity is stable within one frame. */
  current(): SnapshotResult<T> {
    return this.values
  }

  /** Subscribe to per-frame change notifications. Returns an
   *  unsubscribe thunk. Must be paired with `start()` once. */
  subscribe(cb: () => void): () => void {
    this.listeners.add(cb)
    return () => {
      this.listeners.delete(cb)
    }
  }

  /** Wire each underlying store. Call once before the first
   *  `subscribe`; returns a dispose thunk that unsubscribes from every
   *  store and cancels any pending flush. */
  start(): () => void {
    if (this.storeUnsubs) {
      throw new Error('FrameSnapshot.start() called twice without dispose')
    }
    this.storeUnsubs = this.entries.map(([store]) =>
      store.subscribe(() => this.scheduleFlush()),
    )
    return () => {
      this.cancelScheduled?.()
      this.cancelScheduled = null
      this.pending = false
      this.storeUnsubs?.forEach((u) => u())
      this.storeUnsubs = null
    }
  }

  /** Internal — coalesce N rapid notifications into one rAF flush. */
  private scheduleFlush(): void {
    if (this.pending) return
    this.pending = true
    this.cancelScheduled = this.schedule(() => this.flush())
  }

  /** Internal — recompute, diff, and notify listeners. Exposed only
   *  via the schedule callback under normal use; surfaced as a method
   *  so tests can drive the cycle deterministically. */
  flush(): void {
    this.pending = false
    this.cancelScheduled = null
    const next = this.read()
    const prev = this.values
    if (!sameTuple(prev, next)) {
      this.values = next
      this.listeners.forEach((l) => l())
    }
  }
}

function sameTuple(a: readonly unknown[], b: readonly unknown[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (!Object.is(a[i], b[i])) return false
  }
  return true
}
