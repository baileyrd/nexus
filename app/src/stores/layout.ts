import { create } from "zustand";
import {
  getDefaultLayout,
  getLayoutPreset,
  listLayoutPresets,
  type PresetInfo,
  type WorkspaceLayout,
} from "../ipc/layout";

interface LayoutState {
  layout: WorkspaceLayout | null;
  presets: PresetInfo[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  loadPresetList: () => Promise<void>;
  loadPreset: (id: string) => Promise<void>;
}

export const useLayoutStore = create<LayoutState>((set) => ({
  layout: null,
  presets: [],
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

  loadPresetList: async () => {
    try {
      const presets = await listLayoutPresets();
      set({ presets });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadPreset: async (id: string) => {
    set({ loading: true, error: null });
    try {
      const layout = await getLayoutPreset(id);
      set({ layout, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));
