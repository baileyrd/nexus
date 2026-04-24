// shell/src/plugins/core/capabilityPrompt/capabilityPromptStore.ts
//
// WI-31 — Prompt queue state.
//
// One modal at a time (blocking high-risk consent) + any number of
// concurrent banners (low/medium banners can stack). Plugins land in
// the queue in scan order; the modal advances through them FIFO so the
// user approves/denies each one in turn before the plugin activates.
//
// Denied plugins stay in `denied` — the PluginsMgmt row surfaces them
// as "denied" so the user can open the prompt again from the settings
// UI (follow-up iteration handles explicit revocation).

import { create } from 'zustand'
import type { Capability } from '@nexus/extension-api'

/** One pending modal prompt (high-risk consent). */
export interface ModalPrompt {
  pluginId: string
  pluginName: string
  version: string
  pluginDir: string
  /** Full declared capability list (all risk levels). */
  caps: Capability[]
  /** Caps the user previously OK'd (PascalCase). Empty on first install. */
  previouslyGranted: Capability[]
  reason: 'first-install' | 'version-bump' | 'capability-change'
  /** Resolved on Approve (ok: true, caps granted) or Deny (ok: false). */
  resolve: (result: { ok: boolean; grantedCaps: Capability[] }) => void
}

/** One non-blocking banner (low/medium-only declaration). */
export interface Banner {
  id: number
  pluginId: string
  pluginName: string
  caps: Capability[]
  /** Epoch-ms when the banner was raised — drives auto-dismiss. */
  raisedAt: number
}

interface CapabilityPromptState {
  /** Head of the modal queue, or null when idle. */
  currentModal: ModalPrompt | null
  /** Pending FIFO queue behind `currentModal`. */
  queue: ModalPrompt[]
  /** Active non-blocking banners (no ordering constraint). */
  banners: Banner[]
  /** Plugin ids the user explicitly denied this session. */
  denied: Set<string>

  enqueueModal(p: ModalPrompt): void
  resolveCurrent(ok: boolean, grantedCaps: Capability[]): void
  pushBanner(b: Omit<Banner, 'id' | 'raisedAt'>): number
  dismissBanner(id: number): void
}

let nextBannerId = 1

export const useCapabilityPromptStore = create<CapabilityPromptState>(
  (set, get) => ({
    currentModal: null,
    queue: [],
    banners: [],
    denied: new Set<string>(),

    enqueueModal(p) {
      const s = get()
      if (s.currentModal === null) {
        set({ currentModal: p })
      } else {
        set({ queue: [...s.queue, p] })
      }
    },

    resolveCurrent(ok, grantedCaps) {
      const s = get()
      if (!s.currentModal) return
      const cur = s.currentModal
      cur.resolve({ ok, grantedCaps })
      const nextDenied = new Set(s.denied)
      if (!ok) nextDenied.add(cur.pluginId)
      const [next, ...rest] = s.queue
      set({
        currentModal: next ?? null,
        queue: rest,
        denied: nextDenied,
      })
    },

    pushBanner(b) {
      const id = nextBannerId++
      const full: Banner = { ...b, id, raisedAt: Date.now() }
      set((s) => ({ banners: [...s.banners, full] }))
      return id
    },

    dismissBanner(id) {
      set((s) => ({ banners: s.banners.filter((b) => b.id !== id) }))
    },
  }),
)
