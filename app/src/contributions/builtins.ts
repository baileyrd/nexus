import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileTree } from "../components/panels/FileTree";
import { Outline } from "../components/panels/Outline";
import { GeneralTab } from "../components/settings/tabs/GeneralTab";
import { HotkeysTab } from "../components/settings/tabs/HotkeysTab";
import { PluginsTab } from "../components/settings/tabs/PluginsTab";
import { useForgeStore } from "../stores/forge";
import { usePaletteStore } from "../stores/palette";
import { useSettingsStore } from "../stores/settings";
import { contributions } from "./registry";

/**
 * Seeds the contribution registry with commands the workspace frame
 * itself owns — help, settings, command palette entry point — plus the
 * `files` and `outline` content-types. Other panel content-types
 * (`bookmarks`, `commands`, `processes`) are left unregistered;
 * they'll be contributed by their owning PRDs as those land.
 *
 * Idempotent — safe to call more than once during dev hot-reloads.
 */
export function registerBuiltins(): void {
  contributions.registerContentType("files", FileTree);
  contributions.registerContentType("outline", Outline);

  contributions.registerSettingsTab({
    id: "general",
    title: "General",
    icon: "settings",
    group: "options",
    component: GeneralTab,
    order: 10,
  });
  contributions.registerSettingsTab({
    id: "hotkeys",
    title: "Hotkeys",
    icon: "command",
    group: "options",
    component: HotkeysTab,
    order: 20,
  });
  contributions.registerSettingsTab({
    id: "plugins",
    title: "Plugins",
    icon: "plug",
    group: "plugins",
    component: PluginsTab,
    order: 10,
  });

  contributions.registerCommand("workspace.help", () => {
    // Placeholder — real help surface (docs site, in-app help panel) TBD.
    // eslint-disable-next-line no-alert
    alert("Nexus help — documentation surface coming soon.");
  });
  contributions.registerPaletteCommand({
    id: "workspace.help",
    commandId: "workspace.help",
    title: "Help: Show documentation",
    category: "Workspace",
    icon: "help-circle",
  });

  contributions.registerCommand("workspace.settings", () => {
    useSettingsStore.getState().openSettings();
  });
  contributions.registerPaletteCommand({
    id: "workspace.settings",
    commandId: "workspace.settings",
    title: "Settings: Open",
    category: "Workspace",
    icon: "settings",
    keybinding: "Mod+,",
  });

  contributions.registerCommand("workspace.command-palette", () => {
    usePaletteStore.getState().togglePalette();
  });
  // Register the palette's own toggle through the contribution
  // registry so `KeybindingDispatcher` drives Mod+K like any other
  // command. Keeps the chord user-rebindable via the Hotkeys tab
  // and removes the bespoke capture-phase listener that used to
  // live in `CommandPalette.tsx`.
  contributions.registerPaletteCommand({
    id: "workspace.command-palette",
    commandId: "workspace.command-palette",
    title: "Command palette: Toggle",
    category: "Workspace",
    icon: "command",
    keybinding: "Mod+K",
  });

  contributions.registerCommand("workspace.switch-forge", async () => {
    const current = useForgeStore.getState().info;
    const picked = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: current?.root,
      title: "Open forge",
    });
    if (typeof picked !== "string") return;
    // forge.open() resets the viewer in-memory and the hydrate effect
    // restores the new forge's last-open file (if any).
    await useForgeStore.getState().open(picked);
  });
  contributions.registerPaletteCommand({
    id: "workspace.switch-forge",
    commandId: "workspace.switch-forge",
    title: "Forge: Open…",
    category: "Workspace",
    icon: "folder",
  });
}
