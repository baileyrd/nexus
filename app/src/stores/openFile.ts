import { create } from "zustand";
import { readForgeFile, type ForgeFile } from "../ipc/forge";

interface OpenFileState {
  file: ForgeFile | null;
  loading: boolean;
  error: string | null;
  open: (relpath: string) => Promise<void>;
  close: () => void;
}

export const useOpenFileStore = create<OpenFileState>((set) => ({
  file: null,
  loading: false,
  error: null,

  open: async (relpath: string) => {
    set({ loading: true, error: null });
    try {
      const file = await readForgeFile(relpath);
      set({ file, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false, file: null });
    }
  },

  close: () => set({ file: null, error: null }),
}));
