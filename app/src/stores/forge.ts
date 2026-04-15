import { create } from "zustand";
import {
  currentForge,
  openForge,
  type ForgeInfo,
} from "../ipc/forge";

interface ForgeState {
  info: ForgeInfo | null;
  loading: boolean;
  error: string | null;
  /** Bumped whenever the backend reports a filesystem change inside the
   *  forge root. Components that cache directory listings should treat
   *  this as an invalidation key. */
  fsVersion: number;
  load: () => Promise<void>;
  open: (path: string) => Promise<void>;
  bumpFsVersion: () => void;
}

export const useForgeStore = create<ForgeState>((set) => ({
  info: null,
  loading: false,
  error: null,
  fsVersion: 0,

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
      set({ info, loading: false, fsVersion: 0 });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  bumpFsVersion: () => set((s) => ({ fsVersion: s.fsVersion + 1 })),
}));
