import { create } from "zustand";
import { readForgeFile, type ForgeFile } from "../ipc/forge";

interface OpenFileState {
  file: ForgeFile | null;
  loading: boolean;
  error: string | null;
  /** Open a file by relpath. Errors surface via `error`. Use this for
   *  explicit user actions. */
  open: (relpath: string) => Promise<void>;
  /** Re-read the currently-open file. If the file has disappeared
   *  externally, close silently rather than surface an error. Use this
   *  for FS-change-triggered refreshes. */
  refresh: () => Promise<void>;
  close: () => void;
}

export const useOpenFileStore = create<OpenFileState>((set, get) => ({
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

  refresh: async () => {
    const current = get().file;
    if (!current) return;
    try {
      const file = await readForgeFile(current.relpath);
      set({ file });
    } catch {
      // File was deleted/renamed externally — close cleanly.
      set({ file: null, error: null });
    }
  },

  close: () => set({ file: null, error: null }),
}));
