import { create } from "zustand";
import {
  applyTheme as ipcApply,
  getAvailableSnippets,
  getAvailableThemes,
  toggleSnippet as ipcToggleSnippet,
  type SnippetMetadata,
  type ThemeMetadata,
} from "../ipc/theme";

type VariableMap = { [key in string]?: string };

interface ThemeState {
  currentThemeId: string | null;
  themes: ThemeMetadata[];
  snippets: SnippetMetadata[];
  variables: VariableMap;
  loading: boolean;
  error: string | null;
  loadAll: () => Promise<void>;
  applyTheme: (id: string) => Promise<void>;
  toggleSnippet: (id: string) => Promise<void>;
}

// Push every CSS variable onto :root so the whole DOM sees the change. Runs
// synchronously — Chromium applies the new cascade on the next paint.
function writeVariables(vars: VariableMap) {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    if (value !== undefined) {
      root.style.setProperty(key, value);
    }
  }
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  currentThemeId: null,
  themes: [],
  snippets: [],
  variables: {},
  loading: false,
  error: null,

  loadAll: async () => {
    set({ loading: true, error: null });
    try {
      const [themes, snippets] = await Promise.all([
        getAvailableThemes(),
        getAvailableSnippets(),
      ]);
      set({ themes, snippets, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  applyTheme: async (id: string) => {
    set({ loading: true, error: null });
    try {
      const applied = await ipcApply(id);
      writeVariables(applied.variables);
      set({
        currentThemeId: applied.id,
        variables: applied.variables,
        loading: false,
      });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  toggleSnippet: async (id: string) => {
    try {
      const enabledIds = await ipcToggleSnippet(id);
      // Recompute variables on the currently-selected theme so the cascade
      // picks up the toggle. If no theme is applied yet, skip — applyTheme
      // will compute when the user picks one.
      const { currentThemeId, snippets } = get();
      const updated = snippets.map((s) => ({
        ...s,
        enabled: enabledIds.includes(s.id),
      }));
      set({ snippets: updated });
      if (currentThemeId) {
        await get().applyTheme(currentThemeId);
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },
}));
