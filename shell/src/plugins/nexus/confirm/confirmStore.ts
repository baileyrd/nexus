import { create } from 'zustand'

/**
 * One pending confirm request. The resolver fires when the user picks
 * a button (Confirm / Cancel) or dismisses (Esc / backdrop).
 *
 * Multiple `api.input.confirm` calls in flight at once are serialised:
 * a request landing while another is open queues behind it; the modal
 * advances through the queue automatically as each resolves. The
 * queue lives on the store too so it survives store-only consumers
 * (no React-context plumbing).
 */
export interface ConfirmRequest {
  id: number
  message: string
  /** Caller-provided override; defaults render as "Confirm" / "Cancel". */
  confirmLabel?: string
  cancelLabel?: string
  /** Tints the confirm button as destructive (uses --risk). */
  danger?: boolean
  resolve: (ok: boolean) => void
}

export interface ConfirmOptions {
  confirmLabel?: string
  cancelLabel?: string
  danger?: boolean
}

interface ConfirmStoreState {
  /** Currently-displayed request, or null when no modal is open. */
  current: ConfirmRequest | null
  /** Pending FIFO queue. */
  queue: ConfirmRequest[]

  enqueue(req: Omit<ConfirmRequest, 'id'>): void
  /** Resolve the current request and advance to the next queued one. */
  resolveCurrent(ok: boolean): void
}

let nextId = 1

export const useConfirmStore = create<ConfirmStoreState>((set, get) => ({
  current: null,
  queue: [],

  enqueue: (req) => {
    const full: ConfirmRequest = { ...req, id: nextId++ }
    const s = get()
    if (s.current === null) {
      set({ current: full })
    } else {
      set({ queue: [...s.queue, full] })
    }
  },

  resolveCurrent: (ok) => {
    const s = get()
    if (!s.current) return
    s.current.resolve(ok)
    const [next, ...rest] = s.queue
    set({ current: next ?? null, queue: rest })
  },
}))

/**
 * Public entry point used by `api.input.confirm` (see host/PluginAPI.ts).
 * Resolves true on confirm, false on cancel / dismiss.
 */
export function requestConfirm(message: string, options: ConfirmOptions = {}): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    useConfirmStore.getState().enqueue({
      message,
      confirmLabel: options.confirmLabel,
      cancelLabel: options.cancelLabel,
      danger: options.danger,
      resolve,
    })
  })
}
