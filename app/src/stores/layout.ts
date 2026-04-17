import { create } from "zustand";
import type {
  LayoutNode,
  PaneId,
  Panel,
  RibbonItem,
  StatusBarItem,
  Tab,
} from "../bindings";
// Deferred access to avoid a top-level circular import (forge → layout → forge).
// ES module live-bindings make this safe to read at call time.
import { useForgeStore } from "./forge";
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
  type PersistedPaneState,
} from "../ipc/persistence";
import type {
  PluginUiPanel,
  PluginUiRibbonItem,
  PluginUiStatusItem,
} from "../ipc/plugins";

interface LayoutState {
  layout: WorkspaceLayout | null;
  presets: PresetInfo[];
  /** Snapshot of persisted state loaded at boot and kept in sync with
   *  in-memory mutations. Null until `load()` finishes. */
  persistence: LayoutPersistence | null;
  /** Latest snapshot of plugin-contributed side panels. Re-applied to
   *  `layout` whenever the store or plugins reload so the merge
   *  survives preset switches + hot-reloads. */
  pluginPanels: PluginUiPanel[];
  /** Latest snapshot of plugin-contributed ribbon icons. Same
   *  re-application rules as `pluginPanels`. */
  pluginRibbon: PluginUiRibbonItem[];
  /** Latest snapshot of plugin-contributed status-bar entries. */
  pluginStatus: PluginUiStatusItem[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  loadPresetList: () => Promise<void>;
  loadPreset: (id: string) => Promise<void>;
  togglePanelVisibility: (side: "left" | "right", panelId: string) => void;
  toggleSidePanelCollapsed: (side: "left" | "right") => void;
  /** Flip the mini-mode (icons-only) rail flag on a side panel. Orthogonal
   *  to `collapsed`: when mini-mode is on the panel shows only its
   *  selector icons, no panel body. PRD-07 §5.1 / §8. */
  toggleSidePanelMiniMode: (side: "left" | "right") => void;
  activatePanel: (side: "left" | "right", panelId: string) => void;
  /** Update proportional sizes on a split node in the pane tree.
   *  Sizes are held in-memory only for now — cross-session persistence
   *  of split sizes is a separate binding-schema change. */
  setSplitSizes: (paneId: PaneId, sizes: number[]) => void;
  /** Make `tabId` the active tab within the leaf pane that owns it. No-op
   *  if the leaf/tab isn't found. */
  setActiveTab: (paneId: PaneId, tabId: string) => void;
  /** Make `paneId` the focused leaf. Drives the focus ring and
   *  determines where new tabs land by default. */
  focusPane: (paneId: PaneId) => void;
  /** Open (or re-activate) a tab backed by a forge file in the focused
   *  leaf. Content-type is encoded as `file:<relpath>` so `PaneView`
   *  can dispatch to the editor surface without a separate field on
   *  `Tab` (which is a generated binding). */
  openTabForFile: (relpath: string, label: string) => void;
  /** Close the tab with id `tabId` from whichever leaf owns it. If it
   *  was active, activates its neighbour. If the tab's contentType
   *  is `file:<relpath>`, releases the matching openFiles entry. */
  closeTab: (tabId: string) => void;
  /** Flip a tab's dirty flag so the tab-strip indicator tracks editor
   *  state. No-op if the tab isn't found. */
  setTabDirty: (tabId: string, isDirty: boolean) => void;
  /** Rebuild file-backed tab lists in the active layout from the forge's
   *  persisted state. Called by `forge.hydrate` after the layout preset
   *  loads. Unknown pane ids in the persisted map are dropped; empty
   *  panes are left untouched. */
  hydrateTabsForForge: (forgeRoot: string) => void;
  /** Replace the plugin-panel snapshot and re-merge into the active
   *  layout. Call after fetching `list_plugin_panels`. */
  setPluginPanels: (panels: PluginUiPanel[]) => void;
  /** Replace the plugin-ribbon snapshot and re-merge into the active
   *  layout. Call after fetching `list_plugin_ribbon_items`. */
  setPluginRibbon: (items: PluginUiRibbonItem[]) => void;
  /** Replace the plugin-status-bar snapshot and re-merge into the
   *  active layout. Call after fetching `list_plugin_status_items`. */
  setPluginStatus: (items: PluginUiStatusItem[]) => void;
  /** Read the persisted UI state for a forge, or `null` if none yet. */
  forgeUiState: (forgePath: string) => ForgeUiState | null;
  /** Merge `patch` into the persisted UI state for a forge and
   *  schedule a save. */
  updateForgeUiState: (forgePath: string, patch: Partial<ForgeUiState>) => void;
  /** Re-read persistence from disk. Called after the backend mutates
   *  backend-managed fields (e.g. recent-forges list after a picker
   *  open) so the in-memory mirror catches up. */
  refreshPersistence: () => Promise<void>;
}

/**
 * Compose a plugin-contributed `Panel` from a `PluginUiPanel`. Each
 * gets a unique contentType + id namespaced under its owning plugin
 * so they can't collide with built-in panels or each other.
 */
function toPanel(p: PluginUiPanel): Panel {
  const id = `plugin:${p.plugin_id}:${p.panel_id}`;
  return {
    id,
    title: p.title,
    icon: p.icon,
    plugin: p.plugin_id,
    visible: false,
    toolbar: [],
    contentType: id,
  };
}

/**
 * Compose a `StatusBarItem` from a `PluginUiStatusItem`. Plugin id
 * namespace keeps ids collision-free; `action` is set only when the
 * contribution declared a `command`, so items without one render as
 * plain counters.
 */
function toStatusItem(s: PluginUiStatusItem): StatusBarItem {
  return {
    id: `plugin:${s.plugin_id}:${s.status_id}`,
    text: s.text ?? s.tooltip ?? null,
    icon: s.icon,
    action: s.command_id
      ? { kind: "invokeCommand", command: s.command_id }
      : null,
    plugin: s.plugin_id,
  };
}

/**
 * Return `layout` with `pluginStatus` merged into its status-bar list.
 * Entries with `plugin != null` are dropped from the base first so
 * repeated merges don't leave stale counters behind.
 */
function mergePluginStatus(
  layout: WorkspaceLayout,
  pluginStatus: PluginUiStatusItem[],
): WorkspaceLayout {
  const builtin = layout.statusBar.filter((s) => !s.plugin);
  const additions = pluginStatus.map(toStatusItem);
  return { ...layout, statusBar: [...builtin, ...additions] };
}

/**
 * Compose a `RibbonItem` from a `PluginUiRibbonItem`. The id is
 * namespaced under its owning plugin so it can't collide with builtin
 * ribbon entries or another plugin's. The action always dispatches
 * via `invokeCommand` using the pre-qualified `command_id`.
 */
function toRibbonItem(r: PluginUiRibbonItem): RibbonItem {
  return {
    id: `plugin:${r.plugin_id}:${r.ribbon_id}`,
    icon: r.icon,
    tooltip: r.tooltip,
    plugin: r.plugin_id,
    action: { kind: "invokeCommand", command: r.command_id },
  };
}

/**
 * Return `layout` with `pluginRibbon` merged into its ribbon list.
 * Ribbon items with `plugin != null` are dropped from the base first
 * so repeated merges (plugin reload) don't leave stale entries.
 */
function mergePluginRibbon(
  layout: WorkspaceLayout,
  pluginRibbon: PluginUiRibbonItem[],
): WorkspaceLayout {
  const builtin = layout.ribbon.filter((r) => !r.plugin);
  const additions = pluginRibbon.map(toRibbonItem);
  return { ...layout, ribbon: [...builtin, ...additions] };
}

/**
 * Return `layout` with `pluginPanels` merged into the correct side.
 * Panels with `plugin != null` are dropped from the base first so
 * repeated merges (plugin reload) don't leave stale entries.
 */
function mergePluginPanels(
  layout: WorkspaceLayout,
  pluginPanels: PluginUiPanel[],
): WorkspaceLayout {
  function mergeSide(
    side: WorkspaceLayout["leftSidePanel"],
    whichSide: "left" | "right",
  ): WorkspaceLayout["leftSidePanel"] {
    const builtin = side.panels.filter((p) => !p.plugin);
    const builtinOrder = side.panelOrder.filter(
      (id) => builtin.some((p) => p.id === id),
    );
    const additions = pluginPanels
      .filter((p) => p.side === whichSide)
      .map(toPanel);
    return {
      ...side,
      panels: [...builtin, ...additions],
      panelOrder: [...builtinOrder, ...additions.map((p) => p.id)],
    };
  }

  return {
    ...layout,
    leftSidePanel: mergeSide(layout.leftSidePanel, "left"),
    rightSidePanel: mergeSide(layout.rightSidePanel, "right"),
  };
}

/** Return a new subtree with the split node matching `paneId` updated
 *  to `sizes`. Returns `node` unchanged (same identity) if the id
 *  isn't found anywhere in the subtree, so callers can detect no-ops. */
function updateSplitSizes(
  node: LayoutNode,
  paneId: PaneId,
  sizes: number[],
): LayoutNode {
  if (node.type === "leaf") return node;
  if (node.id === paneId) {
    if (sizes.length !== node.children.length) return node;
    return { ...node, sizes };
  }
  let changed = false;
  const nextChildren = node.children.map((c) => {
    const updated = updateSplitSizes(c, paneId, sizes);
    if (updated !== c) changed = true;
    return updated;
  });
  return changed ? { ...node, children: nextChildren } : node;
}

type LeafNode = Extract<LayoutNode, { type: "leaf" }>;

/** Basename helper for deriving tab labels from relpaths. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf("/");
  return i === -1 ? relpath : relpath.slice(i + 1);
}

/** Construct a file-backed tab. `id` is randomised so each session's
 *  ids are unique; `contentType = file:<relpath>` drives PaneView's
 *  dispatch to the editor surface. */
function makeFileTab(relpath: string, pinned: boolean): Tab {
  return {
    id: `tab:${relpath}:${Date.now()}:${Math.random().toString(36).slice(2, 6)}`,
    label: basename(relpath) || relpath,
    icon: "file",
    surface: "editor",
    pinned,
    contentType: `file:${relpath}`,
    isDirty: false,
  };
}

/** Current forge root, or null if no forge is open. Safe to call from
 *  inside layout-store actions despite the circular import: the read
 *  happens at action time, not module init. */
function currentForgeRoot(): string | null {
  return useForgeStore.getState().info?.root ?? null;
}

/** Walk the layout tree and project every leaf's file-backed tabs into
 *  the persistence shape. Non-file tabs are ignored. */
function extractPanesFromLayout(node: LayoutNode): Record<string, PersistedPaneState> {
  const out: Record<string, PersistedPaneState> = {};
  function walk(n: LayoutNode) {
    if (n.type === "leaf") {
      const fileTabs = n.tabs.filter((t) => t.contentType.startsWith("file:"));
      if (fileTabs.length === 0) return;
      const activeTab = n.tabs.find((t) => t.id === n.activeTabId);
      const activeRelpath =
        activeTab && activeTab.contentType.startsWith("file:")
          ? activeTab.contentType.slice("file:".length)
          : null;
      out[n.id] = {
        tabs: fileTabs.map((t) => ({
          relpath: t.contentType.slice("file:".length),
          pinned: t.pinned,
        })),
        activeRelpath,
      };
      return;
    }
    for (const c of n.children) walk(c);
  }
  walk(node);
  return out;
}

/** First leaf encountered in document order. */
function firstLeaf(node: LayoutNode): LeafNode | null {
  if (node.type === "leaf") return node;
  for (const c of node.children) {
    const f = firstLeaf(c);
    if (f) return f;
  }
  return null;
}

/** Return the leaf with `paneId`, or null. */
function findLeaf(node: LayoutNode, paneId: PaneId): LeafNode | null {
  if (node.type === "leaf") return node.id === paneId ? node : null;
  for (const c of node.children) {
    const f = findLeaf(c, paneId);
    if (f) return f;
  }
  return null;
}

/** Return the leaf containing `tabId`, or null. */
function findLeafWithTab(node: LayoutNode, tabId: string): LeafNode | null {
  if (node.type === "leaf") {
    return node.tabs.some((t) => t.id === tabId) ? node : null;
  }
  for (const c of node.children) {
    const f = findLeafWithTab(c, tabId);
    if (f) return f;
  }
  return null;
}

/** Rewrite the subtree, replacing the leaf with matching id. Returns the
 *  input unchanged (same identity) if not found. */
function replaceLeaf(
  node: LayoutNode,
  paneId: PaneId,
  update: (leaf: LeafNode) => LeafNode,
): LayoutNode {
  if (node.type === "leaf") {
    if (node.id !== paneId) return node;
    const next = update(node);
    return next === node ? node : next;
  }
  let changed = false;
  const nextChildren = node.children.map((c) => {
    const updated = replaceLeaf(c, paneId, update);
    if (updated !== c) changed = true;
    return updated;
  });
  return changed ? { ...node, children: nextChildren } : node;
}

/** Return a new subtree with the leaf matching `paneId` activating
 *  `tabId`. Identity-preserving if not found or already active. */
function updateActiveTab(
  node: LayoutNode,
  paneId: PaneId,
  tabId: string,
): LayoutNode {
  if (node.type === "leaf") {
    if (node.id !== paneId) return node;
    if (node.activeTabId === tabId) return node;
    if (!node.tabs.some((t) => t.id === tabId)) return node;
    return { ...node, activeTabId: tabId };
  }
  let changed = false;
  const nextChildren = node.children.map((c) => {
    const updated = updateActiveTab(c, paneId, tabId);
    if (updated !== c) changed = true;
    return updated;
  });
  return changed ? { ...node, children: nextChildren } : node;
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

/** Return a new persistence blob with the given layout's tab state
 *  merged into the forge-scoped panes map. Preserves `expandedPaths`
 *  and legacy `openFile`. Returns `null` if no forge is open (nothing
 *  to persist against). */
function persistencePatchForPanes(
  previous: LayoutPersistence | null,
  forgeRoot: string,
  layout: WorkspaceLayout,
): LayoutPersistence {
  const base: LayoutPersistence = previous ?? {
    version: 1,
    lastPresetId: null,
    layouts: {},
  };
  const prevForge: ForgeUiState = base.forgeState?.[forgeRoot] ?? {
    expandedPaths: [],
    openFile: null,
  };
  const nextForge: ForgeUiState = {
    ...prevForge,
    panes: extractPanesFromLayout(layout.root),
  };
  return {
    ...base,
    forgeState: { ...(base.forgeState ?? {}), [forgeRoot]: nextForge },
  };
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
  pluginPanels: [],
  pluginRibbon: [],
  pluginStatus: [],
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
      const overlaid = applyOverlay(base, persistence?.layouts?.[base.id]);
      const withPanels = mergePluginPanels(overlaid, get().pluginPanels);
      const withRibbon = mergePluginRibbon(withPanels, get().pluginRibbon);
      const layout = mergePluginStatus(withRibbon, get().pluginStatus);
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
      const { persistence, pluginPanels, pluginRibbon, pluginStatus } = get();
      const overlaid = applyOverlay(base, persistence?.layouts?.[base.id]);
      const withPanels = mergePluginPanels(overlaid, pluginPanels);
      const withRibbon = mergePluginRibbon(withPanels, pluginRibbon);
      const layout = mergePluginStatus(withRibbon, pluginStatus);
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

  toggleSidePanelMiniMode: (side) =>
    set((state) => {
      if (!state.layout) return {};
      const key = side === "left" ? "leftSidePanel" : "rightSidePanel";
      const sidePanel = state.layout[key];
      const layout = {
        ...state.layout,
        [key]: { ...sidePanel, miniMode: !sidePanel.miniMode },
      };
      const persistence = updatePersistence(state.persistence, layout);
      scheduleSave(persistence);
      return { layout, persistence };
    }),

  // Selector-click semantics: make `panelId` the sole visible panel on
  // that side and ensure the side panel itself is expanded.
  setSplitSizes: (paneId, sizes) =>
    set((state) => {
      if (!state.layout) return {};
      const nextRoot = updateSplitSizes(state.layout.root, paneId, sizes);
      if (nextRoot === state.layout.root) return {};
      return { layout: { ...state.layout, root: nextRoot } };
    }),

  setActiveTab: (paneId, tabId) =>
    set((state) => {
      if (!state.layout) return {};
      const nextRoot = updateActiveTab(state.layout.root, paneId, tabId);
      if (nextRoot === state.layout.root) return {};
      const layout = { ...state.layout, root: nextRoot };
      const root = currentForgeRoot();
      if (!root) return { layout };
      const persistence = persistencePatchForPanes(state.persistence, root, layout);
      scheduleSave(persistence);
      return { layout, persistence };
    }),

  focusPane: (paneId) =>
    set((state) => {
      if (!state.layout) return {};
      if (state.layout.focusedPaneId === paneId) return {};
      // Guard against focusing a paneId that isn't in the tree (stale
      // focus from an earlier preset, say).
      if (!findLeaf(state.layout.root, paneId)) return {};
      return { layout: { ...state.layout, focusedPaneId: paneId } };
    }),

  openTabForFile: (relpath, label) => {
    set((state) => {
      if (!state.layout) return {};
      const contentType = `file:${relpath}`;
      // Target: focused leaf if still present, else first leaf.
      const focusedId = state.layout.focusedPaneId ?? null;
      const focused = focusedId ? findLeaf(state.layout.root, focusedId) : null;
      const target = focused ?? firstLeaf(state.layout.root);
      if (!target) return {};

      // Already open in that leaf? Just activate.
      const existing = target.tabs.find((t) => t.contentType === contentType);
      if (existing) {
        if (target.activeTabId === existing.id) return {};
        const nextRoot = replaceLeaf(state.layout.root, target.id, (leaf) => ({
          ...leaf,
          activeTabId: existing.id,
        }));
        return { layout: { ...state.layout, root: nextRoot } };
      }

      // Otherwise push a new tab and activate it.
      const newTab = makeFileTab(relpath, false);
      // `label` is accepted for compatibility with callers that know the
      // intended display name before the file is loaded; we derive from
      // the relpath to keep restore behavior consistent.
      newTab.label = label || newTab.label;
      const nextRoot = replaceLeaf(state.layout.root, target.id, (leaf) => ({
        ...leaf,
        tabs: [...leaf.tabs, newTab],
        activeTabId: newTab.id,
      }));
      const layout = {
        ...state.layout,
        focusedPaneId: target.id,
        root: nextRoot,
      };
      const root = currentForgeRoot();
      if (!root) return { layout };
      const persistence = persistencePatchForPanes(state.persistence, root, layout);
      scheduleSave(persistence);
      return { layout, persistence };
    });
  },

  closeTab: (tabId) =>
    set((state) => {
      if (!state.layout) return {};
      const leaf = findLeafWithTab(state.layout.root, tabId);
      if (!leaf) return {};
      const closing = leaf.tabs.find((t) => t.id === tabId);
      if (closing?.pinned) return {};

      const idx = leaf.tabs.findIndex((t) => t.id === tabId);
      const nextTabs = leaf.tabs.filter((t) => t.id !== tabId);
      let nextActive = leaf.activeTabId ?? null;
      if (nextActive === tabId) {
        const neighbour = nextTabs[idx] ?? nextTabs[idx - 1] ?? null;
        nextActive = neighbour?.id ?? null;
      }

      const nextRoot = replaceLeaf(state.layout.root, leaf.id, (l) => ({
        ...l,
        tabs: nextTabs,
        activeTabId: nextActive,
      }));

      // Free the backing file entry if this tab was the last one pointing
      // at it. Import lazily to avoid a circular module dep with openFiles.
      if (closing?.contentType.startsWith("file:")) {
        const relpath = closing.contentType.slice("file:".length);
        const stillOpenElsewhere = (function check(node: LayoutNode): boolean {
          if (node.type === "leaf") {
            return node.tabs.some(
              (t) => t.id !== tabId && t.contentType === closing.contentType,
            );
          }
          return node.children.some(check);
        })(nextRoot);
        if (!stillOpenElsewhere) {
          void import("./openFiles").then((m) =>
            m.useOpenFilesStore.getState().close(relpath),
          );
        }
      }

      const layout = { ...state.layout, root: nextRoot };
      const root = currentForgeRoot();
      if (!root) return { layout };
      const persistence = persistencePatchForPanes(state.persistence, root, layout);
      scheduleSave(persistence);
      return { layout, persistence };
    }),

  setTabDirty: (tabId, isDirty) =>
    set((state) => {
      if (!state.layout) return {};
      const leaf = findLeafWithTab(state.layout.root, tabId);
      if (!leaf) return {};
      const existing = leaf.tabs.find((t) => t.id === tabId);
      if (!existing || existing.isDirty === isDirty) return {};
      const nextRoot = replaceLeaf(state.layout.root, leaf.id, (l) => ({
        ...l,
        tabs: l.tabs.map((t) => (t.id === tabId ? { ...t, isDirty } : t)),
      }));
      return { layout: { ...state.layout, root: nextRoot } };
    }),

  hydrateTabsForForge: (forgeRoot) =>
    set((state) => {
      if (!state.layout) return {};
      const panes = state.persistence?.forgeState?.[forgeRoot]?.panes;
      if (!panes || Object.keys(panes).length === 0) return {};
      let root = state.layout.root;
      let changed = false;
      for (const [paneId, pane] of Object.entries(panes)) {
        if (!findLeaf(root, paneId as PaneId)) continue;
        if (pane.tabs.length === 0) continue;
        const restored = pane.tabs.map((pt) => makeFileTab(pt.relpath, pt.pinned === true));
        const activeTab = pane.activeRelpath
          ? restored.find((t) => t.contentType === `file:${pane.activeRelpath}`)
          : undefined;
        const activeTabId = activeTab?.id ?? restored[0]?.id ?? null;
        root = replaceLeaf(root, paneId as PaneId, (leaf) => ({
          ...leaf,
          tabs: restored,
          activeTabId,
        }));
        changed = true;
        // Prime the keyed openFiles store so the editor has content the
        // moment the leaf renders.
        for (const pt of pane.tabs) {
          void import("./openFiles").then((m) =>
            m.useOpenFilesStore.getState().open(pt.relpath),
          );
        }
      }
      if (!changed) return {};
      return { layout: { ...state.layout, root } };
    }),

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

  setPluginPanels: (panels) =>
    set((state) => {
      if (!state.layout) return { pluginPanels: panels };
      return {
        pluginPanels: panels,
        layout: mergePluginPanels(state.layout, panels),
      };
    }),

  setPluginRibbon: (items) =>
    set((state) => {
      if (!state.layout) return { pluginRibbon: items };
      return {
        pluginRibbon: items,
        layout: mergePluginRibbon(state.layout, items),
      };
    }),

  setPluginStatus: (items) =>
    set((state) => {
      if (!state.layout) return { pluginStatus: items };
      return {
        pluginStatus: items,
        layout: mergePluginStatus(state.layout, items),
      };
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

  refreshPersistence: async () => {
    try {
      const persistence = await getLayoutPersistence();
      set({ persistence });
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("[layout] refresh failed:", err);
    }
  },
}));
