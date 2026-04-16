import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileTree } from "../components/panels/FileTree";
import { Outline } from "../components/panels/Outline";
import { makeGenericTreePanelFactory } from "../components/panels/GenericTreePanel";
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
  // Install the GenericTreePanel factory so contributions.registerTreeDataProvider
  // can auto-wire the panel component without a circular import in registry.ts.
  contributions.setTreePanelFactory(makeGenericTreePanelFactory);

  // Default file extension handlers. Markdown files open in the editor
  // surface. Canvas and base files have no dedicated surface yet so they
  // fall through to FileViewer — these registrations act as placeholders
  // that plugin-contributed surfaces can replace via their own handler.
  // Extension handlers use the surface content-type id that must be
  // registered separately by the surface plugin.
  contributions.registerFileHandler("md", "editor");
  contributions.registerFileHandler("mdx", "editor");
  // ".canvas" and ".base" are future surfaces — no content-type registered
  // yet, so resolveFileHandlerForPath will return the id but PaneView will
  // fall back to FileViewer until a plugin registers the matching component.

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

  // ── Menu-bar items (PRD-07 §7.5) ─────────────────────────────────────────
  // File menu
  contributions.registerMenuItem({
    id: "menu.file.open-forge",
    label: "Open Forge…",
    commandId: "workspace.switch-forge",
    menu: "File",
    menuOrder: 10,
    order: 10,
  });
  contributions.registerMenuItem({
    id: "menu.file.settings",
    label: "Settings",
    commandId: "workspace.settings",
    menu: "File",
    menuOrder: 10,
    order: 90,
    separatorBefore: true,
  });

  // View menu
  contributions.registerMenuItem({
    id: "menu.view.command-palette",
    label: "Command Palette",
    commandId: "workspace.command-palette",
    menu: "View",
    menuOrder: 30,
    order: 10,
  });

  // Help menu
  contributions.registerMenuItem({
    id: "menu.help.docs",
    label: "Documentation",
    commandId: "workspace.help",
    menu: "Help",
    menuOrder: 90,
    order: 10,
  });
}
