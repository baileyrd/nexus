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
  togglePanelVisibility: (side: "left" | "right", panelId: string) => void;
  toggleSidePanelCollapsed: (side: "left" | "right") => void;
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

  // Local-only: flips the `visible` flag on a panel so the panel selector's
  // `togglePanel` action feels alive. Persisting through IPC is a later
  // piece once the layout-mutation commands land.
  togglePanelVisibility: (side, panelId) =>
    set((state) => {
      if (!state.layout) return {};
      const key = side === "left" ? "leftSidePanel" : "rightSidePanel";
      const sidePanel = state.layout[key];
      const panels = sidePanel.panels.map((p) =>
        p.id === panelId ? { ...p, visible: !p.visible } : p,
      );
      return {
        layout: {
          ...state.layout,
          [key]: { ...sidePanel, panels },
        },
      };
    }),

  // Local-only: flips the `collapsed` flag on a side panel. Matches the
  // chrome toggle buttons at the edges of the workspace chrome.
  toggleSidePanelCollapsed: (side) =>
    set((state) => {
      if (!state.layout) return {};
      const key = side === "left" ? "leftSidePanel" : "rightSidePanel";
      const sidePanel = state.layout[key];
      return {
        layout: {
          ...state.layout,
          [key]: { ...sidePanel, collapsed: !sidePanel.collapsed },
        },
      };
    }),
}));
