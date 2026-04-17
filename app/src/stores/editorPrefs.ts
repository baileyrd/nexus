import { create } from "zustand";

/** Editor-wide preferences (keybinding mode + rendering mode). Persisted
 *  to `localStorage` so they survive reloads before they're migrated to
 *  the Rust-side persistence layer. */

export type KeybindingMode = "default" | "vim" | "emacs";
export type ViewMode = "source" | "live" | "reading";

interface EditorPrefsState {
  keybindingMode: KeybindingMode;
  viewMode: ViewMode;
  setKeybindingMode: (m: KeybindingMode) => void;
  setViewMode: (m: ViewMode) => void;
  cycleViewMode: () => void;
}

const LS_KEY = "nx.editorPrefs.v1";

interface Persisted {
  keybindingMode: KeybindingMode;
  viewMode: ViewMode;
}

function load(): Persisted {
  const fallback: Persisted = { keybindingMode: "default", viewMode: "live" };
  if (typeof window === "undefined") return fallback;
  try {
    const raw = window.localStorage.getItem(LS_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<Persisted>;
    return {
      keybindingMode:
        parsed.keybindingMode === "vim" || parsed.keybindingMode === "emacs"
          ? parsed.keybindingMode
          : "default",
      viewMode:
        parsed.viewMode === "source" || parsed.viewMode === "reading"
          ? parsed.viewMode
          : "live",
    };
  } catch {
    return fallback;
  }
}

function save(p: Persisted) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(LS_KEY, JSON.stringify(p));
  } catch {
    /* quota / privacy mode; ignore */
  }
}

export const useEditorPrefsStore = create<EditorPrefsState>((set, get) => {
  const initial = load();
  return {
    keybindingMode: initial.keybindingMode,
    viewMode: initial.viewMode,
    setKeybindingMode: (m) =>
      set(() => {
        const next = { keybindingMode: m, viewMode: get().viewMode };
        save(next);
        return { keybindingMode: m };
      }),
    setViewMode: (m) =>
      set(() => {
        const next = { keybindingMode: get().keybindingMode, viewMode: m };
        save(next);
        return { viewMode: m };
      }),
    cycleViewMode: () => {
      const order: ViewMode[] = ["live", "source", "reading"];
      const cur = get().viewMode;
      const next = order[(order.indexOf(cur) + 1) % order.length];
      get().setViewMode(next);
    },
  };
});
