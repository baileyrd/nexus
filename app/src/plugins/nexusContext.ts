/**
 * Host API context passed to JS plugin dispatch functions.
 *
 * Wraps Tauri invoke calls so JS plugins can access settings, emit
 * events, and call other plugins without importing Tauri directly.
 */

import {
  contributions,
  type ContribContextMenuItem,
  type EditorBlockType,
  type EditorDecorationProvider,
  type EditorKeybinding,
  type MenuItem,
  type Snippet,
  type TreeDataProvider,
  type UriHandler,
} from "../contributions";
import { invokePluginCommand } from "../ipc/plugins";
import { getPluginSettings } from "../ipc/pluginSettings";
import { publishHostEvent } from "./events";
import { useToastStore, type ToastLevel } from "../stores/toast";

/** Minimal disposable contract mirroring the contribution registry. */
export type Disposable = () => void;

export interface NexusPluginContext {
  /** The plugin's reverse-DNS identifier. */
  pluginId: string;

  /** Read the plugin's current settings. */
  settings: {
    get(): Promise<Record<string, unknown>>;
  };

  /** Publish events to the kernel event bus + frontend. */
  events: {
    emit(typeId: string, payload: unknown): Promise<void>;
  };

  /** Call another plugin's IPC command. */
  ipc: {
    call(
      targetPluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<unknown>;
  };

  /**
   * Editor-surface extension points (PRD-08 §14.1–14.3). Plugins hold
   * onto the returned disposables and call them from `onStop` so their
   * contributions are removed when the plugin is unloaded.
   */
  editor: {
    registerBlockType(type: EditorBlockType): Disposable;
    registerDecorationProvider(
      provider: EditorDecorationProvider,
    ): Disposable;
    registerKeybinding(binding: EditorKeybinding): Disposable;
    /**
     * Register a text-expansion snippet. When the user types `trigger`
     * and presses Tab in the editor, the trigger is replaced with `body`.
     * Use `$CURSOR` in `body` to control final caret placement.
     * Returns a disposable that removes the snippet on plugin stop.
     */
    registerSnippet(snippet: Snippet): Disposable;
  };

  /**
   * Host UI APIs. Lets plugins surface feedback to the user (toasts,
   * future: quick-pick dialogs, input prompts) without importing
   * Tauri directly or writing bespoke React components.
   */
  ui: {
    /**
     * Show an in-app toast notification. Auto-dismissed after ~5 s.
     * `level` controls the colour badge: "info" (default), "warn", or "error".
     */
    notify(level: ToastLevel, message: string): void;

    /**
     * Register a tree-data provider and claim the content-type `viewId`.
     * A generic tree panel is automatically wired up, so the plugin
     * doesn't need to ship a bespoke React component.
     * Returns a disposable that un-registers on plugin stop.
     */
    registerTreeDataProvider(viewId: string, provider: TreeDataProvider): Disposable;

    /**
     * Map a file extension (without leading dot) to a registered content-type
     * id, so opening that file type in the forge picks the plugin's surface
     * instead of the generic FileViewer.
     *
     * Example: `ctx.ui.registerFileHandler("canvas", "com.myorg.canvas.editor")`
     *
     * The `contentType` must be registered separately via
     * `contributions.registerContentType` (or via a `PanelView` auto-wire).
     */
    registerFileHandler(ext: string, contentTypeId: string): Disposable;

    /**
     * Add a context menu item for one or more surface scopes.
     * The item's action dispatches `commandId` through the contribution
     * registry, so the command must be registered separately via
     * `ctx.commands.register` (or `contributions.registerCommand`).
     *
     * Scopes: `"file-tree:file"`, `"file-tree:directory"`, `"file-tree:root"`.
     *
     * Example:
     * ```ts
     * ctx.ui.registerContextMenuItem({
     *   id: "com.myorg.plugin:copy-path",
     *   label: "Copy relative path",
     *   commandId: "com.myorg.plugin:copy-path",
     *   scopes: ["file-tree:file"],
     * });
     * ```
     */
    registerContextMenuItem(item: ContribContextMenuItem): Disposable;

    /**
     * Contribute an item to the application menu bar (PRD-07 §7.5).
     * Specify the top-level pull-down via `item.menu` (e.g. `"File"`,
     * `"View"`, or a custom plugin-defined label). The action dispatches
     * `commandId` through the contribution registry.
     *
     * Example:
     * ```ts
     * ctx.ui.registerMenuItem({
     *   id: "com.myorg.plugin:export",
     *   label: "Export…",
     *   commandId: "com.myorg.plugin:export",
     *   menu: "File",
     *   order: 50,
     * });
     * ```
     */
    registerMenuItem(item: MenuItem): Disposable;

    /**
     * Register a URI handler for an incoming `scheme://…` URL
     * (PRD-04 §1.1 `protocol_handlers`). When the app receives a URL
     * whose scheme matches `handler.scheme`, `handler.handle(url)` is
     * called with the full URL string. Returns a disposable that removes
     * the handler on plugin stop.
     *
     * Example:
     * ```ts
     * ctx.ui.registerUriHandler({
     *   id: "com.myorg.plugin:nexus",
     *   scheme: "nexus",
     *   handle(url) {
     *     const parsed = new URL(url);
     *     // parsed.pathname → "/analyze", parsed.searchParams → ...
     *   },
     * });
     * ```
     */
    registerUriHandler(handler: UriHandler): Disposable;
  };
}

export function createNexusContext(pluginId: string): NexusPluginContext {
  return {
    pluginId,
    settings: {
      get: () => getPluginSettings(pluginId),
    },
    events: {
      emit: (typeId, payload) =>
        publishHostEvent(typeId, payload as Record<string, unknown>),
    },
    ipc: {
      call: (target, cmd, args) =>
        invokePluginCommand(target, cmd, args ?? {}),
    },
    editor: {
      registerBlockType: (type) =>
        contributions.registerEditorBlockType(type),
      registerDecorationProvider: (provider) =>
        contributions.registerEditorDecorationProvider(provider),
      registerKeybinding: (binding) =>
        contributions.registerEditorKeybinding(binding),
      registerSnippet: (snippet) =>
        contributions.registerSnippet(snippet),
    },
    ui: {
      notify: (level, message) => {
        useToastStore.getState().add({ level, message, source: pluginId });
      },
      registerTreeDataProvider: (viewId, provider) =>
        contributions.registerTreeDataProvider(viewId, provider),
      registerFileHandler: (ext, contentTypeId) =>
        contributions.registerFileHandler(ext, contentTypeId),
      registerContextMenuItem: (item) =>
        contributions.registerContextMenuItem(item),
      registerMenuItem: (item) =>
        contributions.registerMenuItem(item),
      registerUriHandler: (handler) =>
        contributions.registerUriHandler(handler),
    },
  };
}
