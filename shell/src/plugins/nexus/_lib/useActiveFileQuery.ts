// Shared React hook for the "fetch something per active markdown file"
// pattern. Used by side-panel plugins (outgoingLinks, tags, …) whose
// view re-queries the kernel every time the active editor tab changes.
//
// The pattern in each consumer was:
//   - subscribe to `useEditorStore`'s `activeRelpath`
//   - useEffect on relpath change with a local `cancelled` flag
//   - read kernel via `getKernel()`; bail if not ready
//   - clear / loading / error / success state machine
//   - return cleanup that sets `cancelled = true`
//
// Each plugin had ~30 lines of identical glue. This hook owns it once.
//
// Scope note: only covers the in-component `useEffect` flavour of the
// active-file-loader pattern. Plugins that drive a zustand store from
// the plugin's `activate()` (backlinks, graph) keep their existing
// shape — that pattern has more moving parts (request-id counters,
// silent-refresh hooks, block filters) and isn't worth retrofitting.

import { useEffect, useState } from 'react'
import { useEditorStore } from '../editor/editorStore'
import { getKernel } from '../files/kernelClient'

/** Minimal slice of the kernel client we pass to the fetcher. */
export interface KernelClient {
  invoke<T = unknown>(pluginId: string, handler: string, args: unknown): Promise<T>
}

/** Result returned to the component. */
export interface ActiveFileQuery<T> {
  /** Latest data, or `initial` when there's no active file or an error. */
  data: T
  /** True while a fetch is in flight. */
  loading: boolean
  /** Error message, or `null` when the last fetch succeeded. */
  error: string | null
  /** The relpath currently driving the query, or `null` when nothing is open. */
  activeRelpath: string | null
}

export interface UseActiveFileQueryOptions<T> {
  /** Run when `activeRelpath` becomes a non-null path. May invoke the
   *  kernel one or more times. Throwing rejects the promise as usual.
   *  Don't capture state in here — call sites read the returned `data`. */
  fetch: (kernel: KernelClient, relpath: string) => Promise<T>
  /** Value used to seed `data` and to reset it on cancel / error / empty file. */
  initial: T
  /** Optional extra dependency keys. The hook always re-runs on
   *  `activeRelpath` change; pass anything else the fetcher captures. */
  extraDeps?: ReadonlyArray<unknown>
}

/**
 * Subscribe to the active file's relpath and run `fetch` against the
 * kernel whenever it changes. The returned `data` flips back to
 * `initial` while a new fetch is in flight or on error.
 */
export function useActiveFileQuery<T>({
  fetch,
  initial,
  extraDeps = [],
}: UseActiveFileQueryOptions<T>): ActiveFileQuery<T> {
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const [data, setData] = useState<T>(initial)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    if (!activeRelpath) {
      setData(initial)
      setError(null)
      setLoading(false)
      return
    }
    const kernel = getKernel()
    if (!kernel) {
      setError('Kernel not ready.')
      return
    }
    setLoading(true)
    setError(null)
    fetch(kernel, activeRelpath)
      .then((result) => {
        if (cancelled) return
        setData(result)
        setLoading(false)
      })
      .catch((err: unknown) => {
        if (cancelled) return
        setData(initial)
        setError(err instanceof Error ? err.message : String(err))
        setLoading(false)
      })
    return () => {
      cancelled = true
    }
    // `fetch` and `initial` are intentionally NOT in the dep array —
    // the contract is "re-run when the active file changes (or the
    // caller bumps `extraDeps`)". Capturing them would force every
    // caller to memoise, which is more friction than the savings
    // justify.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeRelpath, ...extraDeps])

  return { data, loading, error, activeRelpath }
}
