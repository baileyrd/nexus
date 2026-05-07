// shell/src/plugins/nexus/editor/blameStore.ts
//
// BL-079 — single boolean controlling whether the inline-blame
// extension is mounted. Lives in its own Zustand store so a
// command toggle can flip it without React prop drilling; the
// editor view subscribes and remounts CM6 when it flips.

import { create } from 'zustand'

interface BlameState {
  enabled: boolean
  toggle(): void
  setEnabled(v: boolean): void
}

export const useEditorBlameStore = create<BlameState>((set) => ({
  enabled: false,
  toggle: () => set((s) => ({ enabled: !s.enabled })),
  setEnabled: (v) => set({ enabled: v }),
}))
