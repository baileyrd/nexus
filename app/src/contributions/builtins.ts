import { createElement } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileTree } from "../components/panels/FileTree";
import { FileViewer } from "../components/panels/FileViewer";
import { Outline } from "../components/panels/Outline";
import { makeGenericTreePanelFactory } from "../components/panels/GenericTreePanel";
import { GeneralTab } from "../components/settings/tabs/GeneralTab";
import { HotkeysTab } from "../components/settings/tabs/HotkeysTab";
import { PluginsTab } from "../components/settings/tabs/PluginsTab";
import { RunningExtensionsTab } from "../components/settings/tabs/RunningExtensionsTab";
import { useForgeStore } from "../stores/forge";
import { usePaletteStore } from "../stores/palette";
import { useSettingsStore } from "../stores/settings";
import { useEditorPrefsStore, type KeybindingMode, type ViewMode } from "../stores/editorPrefs";
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

  // UI F-1.1.1: dogfood the markdown editor as a content-type contribution
  // rather than a hard-coded fallback in PaneView. `FileViewer` mounts the
  // CM6 editor and is registered under both the canonical `"com.nexus.editor.markdown"`
  // content-type id and a legacy `"editor"` alias for pre-F-1.1.1 code paths.
  // Markdown / MDX file extensions resolve through the same file-handler
  // registry any community plugin can contribute to.
  // Thin wrapper so FileViewer satisfies the `ContentComponent` signature
  // (which passes a Panel prop). The no-arg `FileViewer()` call picks up
  // the legacy single-file path from useOpenFileStore; PaneView handles
  // `file:<relpath>` tabs directly and never reaches this registration.
  const LegacyFileViewer = () => createElement(FileViewer);
  contributions.registerContentType("com.nexus.editor.markdown", LegacyFileViewer);
  contributions.registerContentType("editor", LegacyFileViewer);
  contributions.registerFileHandler("md", "com.nexus.editor.markdown");
  contributions.registerFileHandler("mdx", "com.nexus.editor.markdown");
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
  contributions.registerSettingsTab({
    id: "running-extensions",
    title: "Running extensions",
    icon: "activity",
    group: "plugins",
    component: RunningExtensionsTab,
    order: 20,
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

  // ── Editor mode commands (PRD-08 §15, §9) ────────────────────────────────
  contributions.registerCommand("editor.toggle-mode", () => {
    useEditorPrefsStore.getState().cycleViewMode();
  });
  contributions.registerPaletteCommand({
    id: "editor.toggle-mode",
    commandId: "editor.toggle-mode",
    title: "Editor: Cycle view mode (live → source → reading)",
    category: "Editor",
    icon: "eye",
    keybinding: "Mod+Shift+E",
  });

  for (const mode of ["live", "source", "reading"] as ViewMode[]) {
    const id = `editor.view-mode.${mode}`;
    contributions.registerCommand(id, () => {
      useEditorPrefsStore.getState().setViewMode(mode);
    });
    contributions.registerPaletteCommand({
      id,
      commandId: id,
      title: `Editor: Switch to ${mode} mode`,
      category: "Editor",
      icon: "eye",
    });
  }

  for (const mode of ["default", "vim", "emacs"] as KeybindingMode[]) {
    const id = `editor.keybinding-mode.${mode}`;
    contributions.registerCommand(id, () => {
      useEditorPrefsStore.getState().setKeybindingMode(mode);
    });
    contributions.registerPaletteCommand({
      id,
      commandId: id,
      title: `Editor: Use ${mode} keybindings`,
      category: "Editor",
      icon: "keyboard",
    });
  }

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
