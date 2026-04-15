import { create } from "zustand";
import {
  getDefaultLayout,
  getLayoutPreset,
  listLayoutPresets,
  type PresetInfo,
  type WorkspaceLayout,
} from "../ipc/layout";
import {
  getLayoutPersistence,
  saveLayoutPersistence,
  type ForgeUiState,
  type LayoutPersistence,
  type PersistedLayoutState,
} from "../ipc/persistence";

interface LayoutState {
  layout: WorkspaceLayout | null;
  presets: PresetInfo[];
  /** Snapshot of persisted state loaded at boot and kept in sync with
   *  in-memory mutations. Null until `load()` finishes. */
  persistence: LayoutPersistence | null;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  loadPresetList: () => Promise<void>;
  loadPreset: (id: string) => Promise<void>;
  togglePanelVisibility: (side: "left" | "right", panelId: string) => void;
  toggleSidePanelCollapsed: (side: "left" | "right") => void;
  activatePanel: (side: "left" | "right", panelId: string) => void;
  /** Read the persisted UI state for a forge, or `null` if none yet. */
  forgeUiState: (forgePath: string) => ForgeUiState | null;
  /** Merge `patch` into the persisted UI state for a forge and
   *  schedule a save. */
  updateForgeUiState: (forgePath: string, patch: Partial<ForgeUiState>) => void;
}

/** Merge persisted state over a freshly loaded preset layout. Active-
 *  panel ids that no longer exist in the preset (e.g. preset file was
 *  edited between saves) are silently dropped. */
function applyOverlay(
  layout: WorkspaceLayout,
  state: PersistedLayoutState | undefined,
): WorkspaceLayout {
  if (!state) return layout;

  function applySide(
    side: WorkspaceLayout["leftSidePanel"],
    collapsed: boolean,
    activeId: string | null,
  ): WorkspaceLayout["leftSidePanel"] {
    const hasActive =
      activeId !== null && side.panels.some((p) => p.id === activeId);
    const panels = hasActive
      ? side.panels.map((p) => ({ ...p, visible: p.id === activeId }))
      : side.panels;
    return { ...side, collapsed, panels };
  }

  return {
    ...layout,
    leftSidePanel: applySide(
      layout.leftSidePanel,
      state.leftSidePanelCollapsed,
      state.leftActivePanelId,
    ),
    rightSidePanel: applySide(
      layout.rightSidePanel,
      state.rightSidePanelCollapsed,
      state.rightActivePanelId,
    ),
  };
}

/** Extract the persistable subset of a layout — only fields the UI
 *  mutates today. */
function extractState(layout: WorkspaceLayout): PersistedLayoutState {
  const activeId = (side: WorkspaceLayout["leftSidePanel"]) =>
    side.panels.find((p) => p.visible)?.id ?? null;
  return {
    leftSidePanelCollapsed: layout.leftSidePanel.collapsed,
    rightSidePanelCollapsed: layout.rightSidePanel.collapsed,
    leftActivePanelId: activeId(layout.leftSidePanel),
    rightActivePanelId: activeId(layout.rightSidePanel),
  };
}

/** Debounce writes so a burst of toggles (arrow-key navigation,
 *  repeated clicks) collapses to a single IPC round-trip. */
let saveTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleSave(persistence: LayoutPersistence) {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    saveLayoutPersistence(persistence).catch((err) => {
      // eslint-disable-next-line no-console
      console.warn("[layout] failed to persist state:", err);
    });
  }, 500);
}

/** Build a new persistence blob with the current layout's state
 *  written under its preset id. */
function updatePersistence(
  previous: LayoutPersistence | null,
  layout: WorkspaceLayout,
): LayoutPersistence {
  const base: LayoutPersistence = previous ?? {
    version: 1,
    lastPresetId: layout.id,
    layouts: {},
  };
  return {
    ...base,
    lastPresetId: layout.id,
    layouts: { ...base.layouts, [layout.id]: extractState(layout) },
  };
}

export const useLayoutStore = create<LayoutState>((set, get) => ({
  layout: null,
  presets: [],
  persistence: null,
  loading: false,
  error: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const persistence = await getLayoutPersistence().catch(() => null);
      const presetId = persistence?.lastPresetId ?? null;
      const base = presetId
        ? await getLayoutPreset(presetId).catch(() => getDefaultLayout())
        : await getDefaultLayout();
      const layout = applyOverlay(base, persistence?.layouts?.[base.id]);
      set({
        layout,
        persistence: persistence ?? { version: 1, lastPresetId: null, layouts: {} },
        loading: false,
      });
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
      const base = await getLayoutPreset(id);
      const { persistence } = get();
      const layout = applyOverlay(base, persistence?.layouts?.[base.id]);
      const nextPersistence = updatePersistence(persistence, layout);
      scheduleSave(nextPersistence);
      set({ layout, persistence: nextPersistence, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  togglePanelVisibility: (side, panelId) =>
    set((state) => {
      if (!state.layout) return {};
      const key = side === "left" ? "leftSidePanel" : "rightSidePanel";
      const sidePanel = state.layout[key];
      const panels = sidePanel.panels.map((p) =>
        p.id === panelId ? { ...p, visible: !p.visible } : p,
      );
      const layout = {
        ...state.layout,
        [key]: { ...sidePanel, panels },
      };
      const persistence = updatePersistence(state.persistence, layout);
      scheduleSave(persistence);
      return { layout, persistence };
    }),

  toggleSidePanelCollapsed: (side) =>
    set((state) => {
      if (!state.layout) return {};
      const key = side === "left" ? "leftSidePanel" : "rightSidePanel";
      const sidePanel = state.layout[key];
      const layout = {
        ...state.layout,
        [key]: { ...sidePanel, collapsed: !sidePanel.collapsed },
      };
      const persistence = updatePersistence(state.persistence, layout);
      scheduleSave(persistence);
      return { layout, persistence };
    }),

  // Selector-click semantics: make `panelId` the sole visible panel on
  // that side and ensure the side panel itself is expanded.
  activatePanel: (side, panelId) =>
    set((state) => {
      if (!state.layout) return {};
      const key = side === "left" ? "leftSidePanel" : "rightSidePanel";
      const sidePanel = state.layout[key];
      const panels = sidePanel.panels.map((p) => ({
        ...p,
        visible: p.id === panelId,
      }));
      const layout = {
        ...state.layout,
        [key]: { ...sidePanel, collapsed: false, panels },
      };
      const persistence = updatePersistence(state.persistence, layout);
      scheduleSave(persistence);
      return { layout, persistence };
    }),

  forgeUiState: (forgePath) =>
    get().persistence?.forgeState?.[forgePath] ?? null,

  updateForgeUiState: (forgePath, patch) =>
    set((state) => {
      const base: LayoutPersistence = state.persistence ?? {
        version: 1,
        lastPresetId: null,
        layouts: {},
      };
      const prevForge = base.forgeState?.[forgePath] ?? {
        expandedPaths: [],
        openFile: null,
      };
      const nextForge: ForgeUiState = { ...prevForge, ...patch };
      const persistence: LayoutPersistence = {
        ...base,
        forgeState: { ...(base.forgeState ?? {}), [forgePath]: nextForge },
      };
      // Forge UI updates fire on discrete user actions (folder toggle,
      // file open/close) — write immediately so closing the window
      // right after doesn't lose the change.
      saveLayoutPersistence(persistence).catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("[layout] failed to persist forge state:", err);
      });
      return { persistence };
    }),
}));
