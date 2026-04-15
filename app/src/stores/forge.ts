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
  load: () => Promise<void>;
  open: (path: string) => Promise<void>;
}

export const useForgeStore = create<ForgeState>((set) => ({
  info: null,
  loading: false,
  error: null,

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
      set({ info, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));
