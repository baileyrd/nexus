import { FileTree } from "../components/panels/FileTree";
import { usePaletteStore } from "../stores/palette";
import { contributions } from "./registry";

/**
 * Seeds the contribution registry with commands the workspace frame
 * itself owns — help, settings, command palette entry point — plus the
 * `files` content-type (forge file tree). Other panel content-types
 * (`bookmarks`, `outline`, `commands`, `processes`) are left
 * unregistered; they'll be contributed by their owning PRDs as those
 * land.
 *
 * Idempotent — safe to call more than once during dev hot-reloads.
 */
export function registerBuiltins(): void {
  contributions.registerContentType("files", FileTree);

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
    // eslint-disable-next-line no-alert
    alert("Settings UI — pending (PRD 07 §20).");
  });
  contributions.registerPaletteCommand({
    id: "workspace.settings",
    commandId: "workspace.settings",
    title: "Settings: Open",
    category: "Workspace",
    icon: "settings",
  });

  contributions.registerCommand("workspace.command-palette", () => {
    usePaletteStore.getState().openPalette();
  });
}
