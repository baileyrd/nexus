import { create } from "zustand";
import {
  getDefaultLayout,
  getLayoutPreset,
  type LayoutPresetName,
  type WorkspaceLayout,
} from "../ipc/layout";

interface LayoutState {
  layout: WorkspaceLayout | null;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  loadPreset: (name: LayoutPresetName) => Promise<void>;
}

export const useLayoutStore = create<LayoutState>((set) => ({
  layout: null,
  loading: false,
  error: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const layout = await getDefaultLayout();
      set({ layout, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  loadPreset: async (name: LayoutPresetName) => {
    set({ loading: true, error: null });
    try {
      const layout = await getLayoutPreset(name);
      set({ layout, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));
