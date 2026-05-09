// `api.input.prompt` migrated off `window.prompt` to a styled modal.
//
// Same queue+current shape as confirmStore / pickStore so concurrent
// `prompt` calls serialise behind one modal. Resolves the typed
// string on commit, `null` on cancel — matching the prior
// `window.prompt` contract so callers don't need to change.

import { create } from 'zustand'

interface PendingPrompt {
  id: number
  message: string
  placeholder: string
  initialValue: string
  resolve: (value: string | null) => void
}

interface PromptStoreState {
  current: PendingPrompt | null
  queue: PendingPrompt[]
  enqueue(req: Omit<PendingPrompt, 'id'>): void
  resolveCurrent(value: string | null): void
}

let nextId = 1

export const usePromptStore = create<PromptStoreState>((set, get) => ({
  current: null,
  queue: [],

  enqueue: (req) => {
    const full: PendingPrompt = { ...req, id: nextId++ }
    const s = get()
    if (s.current === null) {
      set({ current: full })
    } else {
      set({ queue: [...s.queue, full] })
    }
  },

  resolveCurrent: (value) => {
    const s = get()
    if (!s.current) return
    s.current.resolve(value)
    const [next, ...rest] = s.queue
    set({ current: next ?? null, queue: rest })
  },
}))

/**
 * Public entry point used by `api.input.prompt`. Resolves the
 * trimmed (no — verbatim — see comment) input string on commit,
 * `null` on cancel / dismiss / Esc / empty submit. Verbatim string
 * matches the prior `window.prompt` shape — callers that want to
 * reject empty input do so themselves (`nexus.files`'s rename flow
 * is the existing precedent).
 */
export function requestPrompt(
  message: string,
  placeholder?: string,
): Promise<string | null> {
  return new Promise<string | null>((resolve) => {
    usePromptStore.getState().enqueue({
      message,
      placeholder: placeholder ?? '',
      initialValue: placeholder ?? '',
      resolve,
    })
  })
}

/** Test-only helper. */
export function _resetPromptStoreForTests(): void {
  usePromptStore.setState({ current: null, queue: [] })
}
