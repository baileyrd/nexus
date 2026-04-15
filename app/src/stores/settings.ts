import { create } from "zustand";
import type { PluginUiSettingsTab } from "../ipc/plugins";

/**
 * Built-in Settings tab ids. Plugin-contributed tabs use a
 * `plugin:<plugin_id>:<tab_id>` namespace, so `activeTab` is widened
 * to `string`.
 */
export type BuiltinSettingsTab = "general" | "hotkeys" | "plugins";

interface SettingsState {
  open: boolean;
  /** Either a built-in tab id (`"general"` / `"hotkeys"` / `"plugins"`)
   *  or a plugin tab id (`"plugin:<plugin_id>:<tab_id>"`). */
  activeTab: string;
  /** Snapshot of plugin-contributed Settings tabs. Populated on boot
   *  and re-synced on every `plugins:reloaded` event. */
  pluginTabs: PluginUiSettingsTab[];
  openSettings: (tab?: string) => void;
  closeSettings: () => void;
  setActiveTab: (tab: string) => void;
  setPluginTabs: (tabs: PluginUiSettingsTab[]) => void;
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
  pluginTabs: [],
  openSettings: (tab) =>
    set((s) => ({ open: true, activeTab: tab ?? s.activeTab })),
  closeSettings: () => set({ open: false }),
  setActiveTab: (tab) => set({ activeTab: tab }),
  setPluginTabs: (tabs) =>
    set((s) => {
      // If the currently-active plugin tab just disappeared (plugin
      // uninstalled or reload removed it), fall back to the aggregate
      // Plugins tab so the modal doesn't get stuck on a dead id.
      const stillExists = !s.activeTab.startsWith("plugin:")
        || tabs.some((t) => settingsTabKey(t) === s.activeTab);
      return {
        pluginTabs: tabs,
        activeTab: stillExists ? s.activeTab : "plugins",
      };
    }),
}));

/** The `activeTab` id used for a plugin-contributed tab. */
export function settingsTabKey(tab: PluginUiSettingsTab): string {
  return `plugin:${tab.plugin_id}:${tab.tab_id}`;
}
