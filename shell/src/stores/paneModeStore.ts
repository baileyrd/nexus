import { create } from 'zustand'

interface PaneModeState {
  /** Id of the paneMode slot entry currently taking over the body, or null. */
  activeViewId: string | null
  /** Raise a pane-mode view. Idempotent — re-entering the same view is a no-op. */
  enter: (viewId: string) => void
  /** Drop back to the tri-pane body. */
  exit: () => void
}

export const usePaneModeStore = create<PaneModeState>((set) => ({
  activeViewId: null,
  enter: (viewId) => set({ activeViewId: viewId }),
  exit: () => set({ activeViewId: null }),
}))
