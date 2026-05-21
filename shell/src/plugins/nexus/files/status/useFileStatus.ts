// BL-053 Phase 4 — convenience hook for the file-tree row.
//
// Returns the cached status for `relpath` (or `null` if we've fetched
// and there's none) and triggers a lazy fetch on first call. The
// hook subscribes to `useStatusStore.revision` so a cache write
// from anywhere (other rows mounting, the FS-event invalidator)
// re-renders rows that read through it.

import { useEffect, useMemo } from 'react'

import type { KernelAPI } from '../../../../types/plugin'
import {
  fetchStatus,
  useStatusStore,
  type CachedStatus,
} from './statusStore'

/** Markdown extensions the dot covers. Non-markdown files won't
 *  trigger a fetch — `read_frontmatter` returns a default-empty
 *  result for them, but the IPC roundtrip is wasted. */
const MARKDOWN_EXTENSIONS = ['md', 'markdown', 'mdx']

/** True when `name`'s extension is in the markdown set. */
export function isMarkdownPath(name: string): boolean {
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  const ext = name.slice(dot + 1).toLowerCase()
  return MARKDOWN_EXTENSIONS.includes(ext)
}

/** Read-through hook. `kernel` may be `null` between
 *  `workspace:closed` and `workspace:opened`; the hook is a no-op
 *  in that window. */
export function useFileStatus(
  relpath: string,
  isDir: boolean,
  name: string,
  kernel: KernelAPI | null,
): CachedStatus | undefined {
  // Subscribe to the revision so a cache mutation triggers a
  // re-render even when our specific key hasn't changed (e.g. the
  // map identity flipped after a FIFO eviction). The selector
  // returns just the entry to avoid the whole-Map subscription.
  const status = useStatusStore((s) => s.cache.get(relpath))

  // Fire-and-forget fetch on mount + whenever the path changes.
  // We deliberately don't kick a fetch when the entry is a directory
  // or non-markdown name — saves an IPC round-trip per row.
  const eligible = useMemo(
    () => !isDir && isMarkdownPath(name),
    [isDir, name],
  )
  useEffect(() => {
    if (!eligible || !kernel) return
    void fetchStatus(kernel, relpath)
  }, [eligible, kernel, relpath])

  return eligible ? status : undefined
}
