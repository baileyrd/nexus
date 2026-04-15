import { create } from "zustand";

export type SettingsTab = "general" | "hotkeys" | "plugins";

interface SettingsState {
  open: boolean;
  activeTab: SettingsTab;
  openSettings: (tab?: SettingsTab) => void;
  closeSettings: () => void;
  setActiveTab: (tab: SettingsTab) => void;
}

/**
 * Controls the Settings modal. The `workspace.settings` contribution
 * command and future deep-linked entry points (e.g. "edit this
 * keybinding") call `openSettings(tab)` to jump directly to the right
 * section.
 */
export const useSettingsStore = create<SettingsState>((set) => ({
  open: false,
  activeTab: "general",
  openSettings: (tab) =>
    set((s) => ({ open: true, activeTab: tab ?? s.activeTab })),
  closeSettings: () => set({ open: false }),
  setActiveTab: (tab) => set({ activeTab: tab }),
}));
