import { create } from 'zustand'

/**
 * Shell-side view-model for `nexus.terminal`.
 *
 * Holds the current session id (assigned when the kernel's
 * `com.nexus.terminal::create_session` returns) and a coarse
 * visibility flag mirrored from layoutStore. `visible` is redundant
 * with `layoutStore.panelArea.visible` but kept here so TerminalView
 * can read a single source without subscribing to the whole layout
 * store.
 */
interface TerminalState {
  sessionId: string | null
  visible: boolean
  setSession(id: string | null): void
  setVisible(v: boolean): void
}

export const useTerminalStore = create<TerminalState>((set) => ({
  sessionId: null,
  visible: false,
  setSession: (id) => set({ sessionId: id }),
  setVisible: (v) => set({ visible: v }),
}))
