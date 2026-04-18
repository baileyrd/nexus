import { createElement } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { BaseViewDemo } from "../components/panels/BaseViewDemo";
import { ChatPanel } from "../components/panels/ChatPanel";
import { FileTree } from "../components/panels/FileTree";
import { SavedCommandsPanel } from "../components/panels/SavedCommandsPanel";
import { FileViewer } from "../components/panels/FileViewer";
import { Outline } from "../components/panels/Outline";
import { TerminalPanel } from "../components/panels/TerminalPanel";
import { makeGenericTreePanelFactory } from "../components/panels/GenericTreePanel";
import { GeneralTab } from "../components/settings/tabs/GeneralTab";
import { HotkeysTab } from "../components/settings/tabs/HotkeysTab";
import { PluginsTab } from "../components/settings/tabs/PluginsTab";
import { RunningExtensionsTab } from "../components/settings/tabs/RunningExtensionsTab";
import { useForgeStore } from "../stores/forge";
import { usePaletteStore } from "../stores/palette";
import { useSettingsStore } from "../stores/settings";
import { useEditorPrefsStore, type KeybindingMode, type ViewMode } from "../stores/editorPrefs";
import { runInlineAi } from "../editor/inlineAi";
import { useLayoutStore } from "../stores/layout";
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

  // PRD-09 §14 terminal surface — thin ContentComponent wrapper around
  // TerminalPanel (which takes no Panel prop of its own, so drop the
  // one PaneView hands us).
  const TerminalSurface = () => createElement(TerminalPanel);
  contributions.registerContentType("terminal", TerminalSurface);

  // PRD-10 §4 database view demo — same wrapper trick for a content
  // type that doesn't need the incoming Panel prop.
  const BaseViewDemoSurface = () => createElement(BaseViewDemo);
  contributions.registerContentType("bases-demo", BaseViewDemoSurface);

  // PRD-12 §6 AI chat surface — thin wrapper, same as Terminal/Bases.
  // The real streaming + provider dispatch live in the `com.nexus.ai`
  // core plugin; the shell just hosts the content-type contribution.
  const ChatSurface = () => createElement(ChatPanel);
  contributions.registerContentType("com.nexus.ai.chat", ChatSurface);

  // PRD-09 §14.1 saved-commands sidebar. Persisted client-side today;
  // the `com.nexus.terminal` plugin will take ownership of the
  // procmgr_commands table in a follow-up slice.
  const SavedCommandsSurface = () => createElement(SavedCommandsPanel);
  contributions.registerContentType(
    "com.nexus.terminal.saved-commands",
    SavedCommandsSurface,
  );

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

  contributions.registerCommand("workspace.open-terminal", () => {
    useLayoutStore
      .getState()
      .openContentTab("terminal", "Terminal", "terminal");
  });
  contributions.registerPaletteCommand({
    id: "workspace.open-terminal",
    commandId: "workspace.open-terminal",
    title: "Terminal: Open",
    category: "Workspace",
    icon: "terminal",
    keybinding: "Mod+Shift+T",
  });

  contributions.registerCommand("workspace.open-saved-commands", () => {
    useLayoutStore
      .getState()
      .openContentTab(
        "com.nexus.terminal.saved-commands",
        "Saved commands",
        "bookmark",
      );
  });
  contributions.registerPaletteCommand({
    id: "workspace.open-saved-commands",
    commandId: "workspace.open-saved-commands",
    title: "Terminal: Saved commands",
    category: "Terminal",
    icon: "bookmark",
  });

  // PRD-08 §9 inline AI — streams a continuation from the AI plugin
  // into the currently-focused CodeMirror view. Context is either the
  // active selection or the preceding ~2 KB of the document.
  contributions.registerCommand("editor.ai-complete", async () => {
    try {
      await runInlineAi();
    } catch (err) {
      // eslint-disable-next-line no-alert
      alert(`Inline AI failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  });
  contributions.registerPaletteCommand({
    id: "editor.ai-complete",
    commandId: "editor.ai-complete",
    title: "AI: Complete at cursor",
    category: "AI",
    icon: "sparkles",
    keybinding: "Mod+Shift+Space",
  });

  contributions.registerCommand("workspace.open-chat", () => {
    useLayoutStore
      .getState()
      .openContentTab("com.nexus.ai.chat", "AI Chat", "message-square");
  });
  contributions.registerPaletteCommand({
    id: "workspace.open-chat",
    commandId: "workspace.open-chat",
    title: "AI: Open chat",
    category: "AI",
    icon: "message-square",
    keybinding: "Mod+Shift+A",
  });

  contributions.registerCommand("workspace.open-bases-demo", () => {
    useLayoutStore
      .getState()
      .openContentTab("bases-demo", "Bases demo", "database");
  });
  contributions.registerPaletteCommand({
    id: "workspace.open-bases-demo",
    commandId: "workspace.open-bases-demo",
    title: "Bases: Open views demo",
    category: "Workspace",
    icon: "database",
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

  // ── Side-panel commands (PRD-07 §5.1, §8) ────────────────────────────────
  // Toggle collapsed (fully hidden) vs. toggle mini-mode (icons-only rail).
  // Exposed as palette commands so users can rebind them and so plugins
  // can invoke them via the same `contributions.invokeCommand` path.
  for (const side of ["left", "right"] as const) {
    const collapseId = `workspace.toggle-${side}-sidebar`;
    contributions.registerCommand(collapseId, () => {
      useLayoutStore.getState().toggleSidePanelCollapsed(side);
    });
    contributions.registerPaletteCommand({
      id: collapseId,
      commandId: collapseId,
      title: `View: Toggle ${side} side panel`,
      category: "View",
      icon: side === "left" ? "panel-left" : "panel-right",
    });

    const miniId = `workspace.toggle-${side}-mini-mode`;
    contributions.registerCommand(miniId, () => {
      useLayoutStore.getState().toggleSidePanelMiniMode(side);
    });
    contributions.registerPaletteCommand({
      id: miniId,
      commandId: miniId,
      title: `View: Toggle ${side} side panel mini-mode`,
      category: "View",
      icon: "minimize-2",
    });

    contributions.registerMenuItem({
      id: `menu.view.toggle-${side}-sidebar`,
      label: `Toggle ${side[0]!.toUpperCase()}${side.slice(1)} Side Panel`,
      commandId: collapseId,
      menu: "View",
      menuOrder: 30,
      order: side === "left" ? 20 : 21,
    });
    contributions.registerMenuItem({
      id: `menu.view.toggle-${side}-mini`,
      label: `Toggle ${side[0]!.toUpperCase()}${side.slice(1)} Mini-Mode`,
      commandId: miniId,
      menu: "View",
      menuOrder: 30,
      order: side === "left" ? 30 : 31,
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

  // ── MDX components (PRD-08 §7) ───────────────────────────────────────────
  // Built-in components render through the same declarative PanelNode
  // dispatcher that ships plugin webview / panel primitives, so no
  // runtime JSX evaluation happens — preserving the strict CSP
  // (UI F-5.1.2, no `'unsafe-eval'`). Plugins can override any of these
  // by registering the same `name` earlier, or add new names through
  // `ctx.editor.registerMdxComponent`.

  // <Card title="..." accent="...">...</Card>
  // Both self-closing (`<Card title="Hi" />`) and block-form with
  // nested text children are supported — the CM6 scanner passes the
  // inner text as `body` when the tag wraps content.
  contributions.registerMdxComponent({
    id: "builtin:Card",
    name: "Card",
    description: "Boxed container with an optional title, used for call-outs and summaries.",
    render: (props) => {
      const title = typeof props.title === "string" ? props.title : undefined;
      const body = typeof props.body === "string" ? props.body : undefined;
      return {
        type: "vstack",
        gap: 6,
        children: [
          ...(title ? [{ type: "heading" as const, value: title, level: 3 as const }] : []),
          ...(body ? [{ type: "markdown" as const, value: body }] : []),
        ],
      };
    },
  });

  // <Callout type="info|warn|error|note">body</Callout>
  contributions.registerMdxComponent({
    id: "builtin:Callout",
    name: "Callout",
    description: "Highlighted inline note with a severity label.",
    render: (props) => {
      const kind = typeof props.type === "string" ? props.type : "note";
      const body = typeof props.body === "string" ? props.body : "";
      return {
        type: "hstack",
        gap: 8,
        children: [
          { type: "text", value: `[${kind.toUpperCase()}]`, strong: true },
          { type: "markdown", value: body },
        ],
      };
    },
  });

  // <Alert level="info|warning|danger">message</Alert>
  contributions.registerMdxComponent({
    id: "builtin:Alert",
    name: "Alert",
    description: "Heavyweight notice block — renders as a heading-level banner.",
    render: (props) => {
      const level = typeof props.level === "string" ? props.level : "info";
      const message =
        typeof props.message === "string"
          ? props.message
          : typeof props.body === "string"
            ? props.body
            : "";
      return {
        type: "vstack",
        gap: 4,
        children: [
          { type: "heading", value: `${level.toUpperCase()} ALERT`, level: 3 },
          { type: "markdown", value: message },
        ],
      };
    },
  });

  // <Badge label="..." /> — tiny status chip.
  contributions.registerMdxComponent({
    id: "builtin:Badge",
    name: "Badge",
    description: "Inline status chip.",
    render: (props) => ({
      type: "text",
      value: typeof props.label === "string" ? `[${props.label}]` : "[badge]",
      strong: true,
    }),
  });
}
