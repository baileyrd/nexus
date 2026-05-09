// BL-077 follow-up — list-picker modal store.
//
// `api.input.pick(items, options)` resolves to one of the supplied
// items (or null on cancel). The store mirrors `confirmStore`'s
// queue+current pattern so multiple `pick` calls in flight serialise
// behind a single modal — no overlap.
//
// We deliberately don't lean on `commandPaletteStore` because the
// palette is purpose-built for command IDs (with command registry
// integration); this is a generic "pick from a list" surface.

import { create } from 'zustand'

export interface PickItem<T = unknown> {
  /** Primary label — top line of each row. */
  label: string
  /** Optional dim secondary text — right-aligned alongside the label. */
  description?: string
  /** Optional small bottom-line — useful for code-action kinds /
   *  filenames / etc. */
  detail?: string
  /** The opaque payload returned to the caller on selection. */
  value: T
}

export interface PickOptions {
  /** Placeholder shown in the filter input when empty. Defaults to
   *  "Type to filter…". */
  placeholder?: string
  /** Title rendered above the input. */
  title?: string
}

interface PendingPick {
  id: number
  items: PickItem[]
  placeholder?: string
  title?: string
  resolve: (item: PickItem | null) => void
}

interface PickStoreState {
  current: PendingPick | null
  queue: PendingPick[]
  enqueue(req: Omit<PendingPick, 'id'>): void
  /** Resolve the current request with the picked item (or null on
   *  cancel) and advance to the next queued one. */
  resolveCurrent(item: PickItem | null): void
}

let nextId = 1

export const usePickStore = create<PickStoreState>((set, get) => ({
  current: null,
  queue: [],

  enqueue: (req) => {
    const full: PendingPick = { ...req, id: nextId++ }
    const s = get()
    if (s.current === null) {
      set({ current: full })
    } else {
      set({ queue: [...s.queue, full] })
    }
  },

  resolveCurrent: (item) => {
    const s = get()
    if (!s.current) return
    s.current.resolve(item)
    const [next, ...rest] = s.queue
    set({ current: next ?? null, queue: rest })
  },
}))

/**
 * Public entry point used by `api.input.pick`. Resolves the picked
 * item's `value` (typed `T`) on selection, `null` on cancel /
 * dismiss.
 *
 * Empty `items` is a no-op — resolves immediately with `null` so
 * callers don't need to guard.
 */
export function requestPick<T>(
  items: PickItem<T>[],
  options: PickOptions = {},
): Promise<T | null> {
  if (items.length === 0) return Promise.resolve(null)
  return new Promise<T | null>((resolve) => {
    usePickStore.getState().enqueue({
      // Cast: store stores `unknown` to keep zustand types tractable;
      // resolver below narrows back to T.
      items: items as PickItem[],
      placeholder: options.placeholder,
      title: options.title,
      resolve: (picked) => {
        resolve(picked === null ? null : (picked.value as T))
      },
    })
  })
}

/** Test-only helper. */
export function _resetPickStoreForTests(): void {
  usePickStore.setState({ current: null, queue: [] })
}
