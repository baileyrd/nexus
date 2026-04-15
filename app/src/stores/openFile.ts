import { create } from "zustand";
import { readForgeFile, type ForgeFile } from "../ipc/forge";
import { HostTopics, publishHostEvent } from "../plugins/events";
import { useForgeStore } from "./forge";
import { useLayoutStore } from "./layout";

interface OpenFileState {
  file: ForgeFile | null;
  loading: boolean;
  error: string | null;
  /** Open a file by relpath. Errors surface via `error`. Use this for
   *  explicit user actions. Persists openFile under the active forge. */
  open: (relpath: string) => Promise<void>;
  /** Re-read the currently-open file. If the file has disappeared
   *  externally, close silently. Used for FS-change refreshes; does
   *  not touch persistence. */
  refresh: () => Promise<void>;
  /** User-initiated close. Clears state and persists openFile=null
   *  under the active forge. */
  close: () => void;
  /** In-memory clear without touching persistence. Used by forge.open
   *  during a forge switch so the OLD forge's last-open isn't
   *  clobbered with null. */
  reset: () => void;
}

/** Persist the current openFile relpath under the active forge, if any. */
function persist(relpath: string | null) {
  const root = useForgeStore.getState().info?.root;
  if (root) {
    useLayoutStore.getState().updateForgeUiState(root, { openFile: relpath });
  }
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
      persist(relpath);
      void publishHostEvent(HostTopics.fileOpened, {
        relpath: file.relpath,
        name: file.name,
      });
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
      // File was deleted/renamed externally — close in memory only;
      // persistence is updated separately when the user acts.
      set({ file: null, error: null });
    }
  },

  close: () => {
    const previous = get().file?.relpath ?? null;
    set({ file: null, error: null });
    persist(null);
    if (previous !== null) {
      void publishHostEvent(HostTopics.fileClosed, { relpath: previous });
    }
  },

  reset: () => set({ file: null, error: null }),
}));
