// Store driving the "New base…" modal. Same pattern as
// confirmStore: a plugin-side helper pushes a request, the modal
// reads `current`, the modal resolves the Promise on submit/cancel.

import { create } from 'zustand'

export interface NewBaseRequest {
  id: number
  defaultParent: string
  resolve: (result: { relpath: string } | null) => void
}

interface NewBaseStore {
  current: NewBaseRequest | null
  request(defaultParent: string): Promise<{ relpath: string } | null>
  resolveCurrent(result: { relpath: string } | null): void
}

let nextId = 1

export const useNewBaseStore = create<NewBaseStore>((set, get) => ({
  current: null,
  request(defaultParent) {
    return new Promise((resolve) => {
      const id = nextId++
      set({ current: { id, defaultParent, resolve } })
    })
  },
  resolveCurrent(result) {
    const cur = get().current
    if (!cur) return
    cur.resolve(result)
    set({ current: null })
  },
}))
