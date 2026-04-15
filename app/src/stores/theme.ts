import { create } from "zustand";
import {
  applyTheme as ipcApply,
  getAvailableSnippets,
  getAvailableThemes,
  setMode as ipcSetMode,
  toggleSnippet as ipcToggleSnippet,
  type SnippetMetadata,
  type ThemeMetadata,
  type ThemeMode,
} from "../ipc/theme";
import { HostTopics, publishHostEvent } from "../plugins/events";

type VariableMap = { [key in string]?: string };

interface ThemeState {
  currentThemeId: string | null;
  themes: ThemeMetadata[];
  snippets: SnippetMetadata[];
  variables: VariableMap;
  mode: ThemeMode;
  loading: boolean;
  error: string | null;
  loadAll: () => Promise<void>;
  applyTheme: (id: string) => Promise<void>;
  setMode: (mode: ThemeMode) => Promise<void>;
  toggleSnippet: (id: string) => Promise<void>;
}

function prefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

// Map the tri-state ThemeMode into a concrete built-in theme id.
function themeIdForMode(mode: ThemeMode): string {
  if (mode === "dark") return "nexus-dark";
  if (mode === "light") return "nexus-light";
  return prefersDark() ? "nexus-dark" : "nexus-light";
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
  mode: "system",
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
      void publishHostEvent(HostTopics.themeChanged, { theme_id: applied.id });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  setMode: async (mode: ThemeMode) => {
    // Persist mode on the Rust side so config() reflects it, then apply the
    // appropriate built-in theme so the variable cascade actually flips.
    try {
      await ipcSetMode(mode);
    } catch (e) {
      set({ error: String(e) });
      return;
    }
    set({ mode });
    await get().applyTheme(themeIdForMode(mode));
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
