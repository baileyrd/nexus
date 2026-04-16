import { create } from "zustand";
import { readForgeFile, writeForgeFile, type ForgeFile } from "../ipc/forge";
import { editorClose, editorSyncContent } from "../ipc/editor";
import { HostTopics, publishHostEvent } from "../plugins/events";
import { useForgeStore } from "./forge";
import { useLayoutStore } from "./layout";

interface OpenFileState {
  file: ForgeFile | null;
  loading: boolean;
  error: string | null;
  /** Whether the editor content has diverged from the saved file. */
  isDirty: boolean;
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
  /** Mark the file as dirty (editor content changed). */
  markDirty: () => void;
  /** Save the current editor content to disk. */
  save: (content: string) => Promise<void>;
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
  isDirty: false,

  open: async (relpath: string) => {
    set({ loading: true, error: null, isDirty: false });
    try {
      const file = await readForgeFile(relpath);
      set({ file, loading: false });
      persist(relpath);
      // Seed the Rust block tree with the freshly-read content so AI / MCP
      // consumers get an accurate parse immediately on open (not after the
      // first 800 ms debounce fires).
      void editorSyncContent(file.relpath, file.content);
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
    // Don't clobber dirty editor content with the disk version.
    if (get().isDirty) return;
    try {
      const file = await readForgeFile(current.relpath);
      set({ file });
    } catch {
      set({ file: null, error: null, isDirty: false });
    }
  },

  close: () => {
    const previous = get().file?.relpath ?? null;
    set({ file: null, error: null, isDirty: false });
    persist(null);
    if (previous !== null) {
      void editorClose(previous);
      void publishHostEvent(HostTopics.fileClosed, { relpath: previous });
    }
  },

  reset: () => set({ file: null, error: null, isDirty: false }),

  markDirty: () => set({ isDirty: true }),

  save: async (content: string) => {
    const file = get().file;
    if (!file) return;
    try {
      await writeForgeFile(file.relpath, content);
      set({ isDirty: false, file: { ...file, content } });
    } catch (e) {
      // eslint-disable-next-line no-console
      console.error(`[save] failed for ${file.relpath}:`, e);
    }
  },
}));
