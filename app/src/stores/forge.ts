import { create } from "zustand";
import {
  currentForge,
  openForge,
  type ForgeInfo,
} from "../ipc/forge";
import { useLayoutStore } from "./layout";
import { useOpenFileStore } from "./openFile";

interface ForgeState {
  info: ForgeInfo | null;
  loading: boolean;
  error: string | null;
  /** Bumped whenever the backend reports a filesystem change inside the
   *  forge root. Components that cache directory listings should treat
   *  this as an invalidation key. */
  fsVersion: number;
  /** Relpaths the user has expanded in the file tree. Mirrored to disk
   *  via the layout store's persistence so the tree restores on
   *  relaunch. */
  expandedPaths: Set<string>;
  load: () => Promise<void>;
  open: (path: string) => Promise<void>;
  bumpFsVersion: () => void;
  setExpanded: (relpath: string, expanded: boolean) => void;
  /** Restore expanded paths and re-open the last viewed file from the
   *  layout-store persistence for the active forge. Safe to call once
   *  both `info` and the layout store's `persistence` are populated. */
  hydrate: () => void;
}

export const useForgeStore = create<ForgeState>((set, get) => ({
  info: null,
  loading: false,
  error: null,
  fsVersion: 0,
  expandedPaths: new Set(),

  load: async () => {
    set({ loading: true, error: null });
    try {
      const info = await currentForge();
      set({ info, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  open: async (path: string) => {
    set({ loading: true, error: null });
    try {
      const info = await openForge(path);
      // Switching forges drops in-memory tree state; the next hydrate
      // call restores anything saved for this forge.
      set({ info, loading: false, fsVersion: 0, expandedPaths: new Set() });
      // Drop the previously-open file from the viewer without
      // persisting null over the OLD forge's saved state — hydrate
      // will re-open the new forge's last file (or leave it closed).
      useOpenFileStore.getState().reset();
      // Backend updated last_forge_path + recent_forge_paths on disk;
      // pull those back into the in-memory mirror so the recent list
      // reflects the new ordering without a restart.
      void useLayoutStore.getState().refreshPersistence();
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  bumpFsVersion: () => set((s) => ({ fsVersion: s.fsVersion + 1 })),

  setExpanded: (relpath, expanded) => {
    const next = new Set(get().expandedPaths);
    if (expanded) next.add(relpath);
    else next.delete(relpath);
    set({ expandedPaths: next });
    const root = get().info?.root;
    if (root) {
      useLayoutStore
        .getState()
        .updateForgeUiState(root, { expandedPaths: Array.from(next) });
    }
  },

  hydrate: () => {
    const info = get().info;
    if (!info) return;
    const ui = useLayoutStore.getState().forgeUiState(info.root);
    if (!ui) return;
    set({ expandedPaths: new Set(ui.expandedPaths) });
    if (ui.openFile) {
      void useOpenFileStore.getState().open(ui.openFile);
    }
  },
}));
