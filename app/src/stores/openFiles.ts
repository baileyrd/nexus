import { create } from "zustand";
import { readForgeFile, writeForgeFile, type ForgeFile } from "../ipc/forge";
import { editorClose, editorSyncContent } from "../ipc/editor";
import { HostTopics, publishHostEvent } from "../plugins/events";

/**
 * Keyed per-file store backing the multi-tab editor. Each open file
 * has an independent entry (content, dirty flag, load/error state) so
 * two tabs can hold two different files simultaneously.
 *
 * `useOpenFileStore` continues to hold the "focused" file for legacy
 * consumers (Outline, plugin bridge); `PaneView` mirrors the active
 * file tab's state into it without re-reading from disk.
 */

export interface OpenFileEntry {
  file: ForgeFile | null;
  loading: boolean;
  error: string | null;
  isDirty: boolean;
}

interface OpenFilesState {
  entries: Record<string, OpenFileEntry>;
  /** Load a file from disk. No-op if already present and loaded. */
  open: (relpath: string) => Promise<OpenFileEntry>;
  /** Drop an entry and notify the Rust editor plugin. */
  close: (relpath: string) => void;
  /** Persist `content` to disk; flip dirty off on success. */
  save: (relpath: string, content: string) => Promise<void>;
  /** Record the latest editor content and mark the entry dirty. */
  setContent: (relpath: string, content: string) => void;
  /** Force a fresh re-read of `relpath` from disk. Preserves dirty=false
   *  so that if the file was edited externally the editor sees it. */
  refresh: (relpath: string) => Promise<void>;
  /** Drop every entry (used when the active forge switches). */
  reset: () => void;
  /** Synchronous read for callers that need a one-shot snapshot. */
  get: (relpath: string) => OpenFileEntry | undefined;
}

const EMPTY: OpenFileEntry = {
  file: null,
  loading: false,
  error: null,
  isDirty: false,
};

export const useOpenFilesStore = create<OpenFilesState>((set, get) => ({
  entries: {},

  open: async (relpath) => {
    const existing = get().entries[relpath];
    if (existing?.file && !existing.loading) return existing;

    set((state) => ({
      entries: {
        ...state.entries,
        [relpath]: { ...EMPTY, ...existing, loading: true, error: null },
      },
    }));

    try {
      const file = await readForgeFile(relpath);
      const entry: OpenFileEntry = {
        file,
        loading: false,
        error: null,
        isDirty: false,
      };
      set((state) => ({
        entries: { ...state.entries, [relpath]: entry },
      }));
      // Seed the Rust block tree immediately so AI/MCP consumers have
      // a parse before the first debounced sync fires.
      void editorSyncContent(file.relpath, file.content);
      void publishHostEvent(HostTopics.fileOpened, {
        relpath: file.relpath,
        name: file.name,
      });
      return entry;
    } catch (e) {
      const entry: OpenFileEntry = {
        file: null,
        loading: false,
        error: String(e),
        isDirty: false,
      };
      set((state) => ({
        entries: { ...state.entries, [relpath]: entry },
      }));
      return entry;
    }
  },

  close: (relpath) => {
    const had = !!get().entries[relpath];
    set((state) => {
      const next = { ...state.entries };
      delete next[relpath];
      return { entries: next };
    });
    if (had) {
      void editorClose(relpath);
      void publishHostEvent(HostTopics.fileClosed, { relpath });
    }
  },

  save: async (relpath, content) => {
    const entry = get().entries[relpath];
    if (!entry?.file) return;
    try {
      await writeForgeFile(relpath, content);
      set((state) => ({
        entries: {
          ...state.entries,
          [relpath]: {
            ...state.entries[relpath],
            isDirty: false,
            file: entry.file ? { ...entry.file, content } : null,
          },
        },
      }));
    } catch (e) {
      // eslint-disable-next-line no-console
      console.error(`[openFiles] save failed for ${relpath}:`, e);
    }
  },

  setContent: (relpath, content) => {
    set((state) => {
      const prev = state.entries[relpath];
      if (!prev?.file) return {};
      // Only mark dirty if content actually diverged from the on-disk copy.
      const isDirty = prev.file.content !== content;
      return {
        entries: {
          ...state.entries,
          [relpath]: { ...prev, isDirty },
        },
      };
    });
  },

  refresh: async (relpath) => {
    const prev = get().entries[relpath];
    if (!prev) return;
    if (prev.isDirty) return;
    try {
      const file = await readForgeFile(relpath);
      set((state) => ({
        entries: {
          ...state.entries,
          [relpath]: { file, loading: false, error: null, isDirty: false },
        },
      }));
    } catch {
      // File disappeared — drop it silently so stale tabs don't linger.
      get().close(relpath);
    }
  },

  reset: () => set({ entries: {} }),

  get: (relpath) => get().entries[relpath],
}));

/** Hook returning the entry for `relpath`, or the empty sentinel. */
export function useOpenFile(relpath: string | null | undefined): OpenFileEntry {
  return useOpenFilesStore((s) =>
    relpath ? s.entries[relpath] ?? EMPTY : EMPTY,
  );
}
