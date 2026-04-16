import { useSyncExternalStore, type ComponentType } from "react";
import type { Extension } from "@codemirror/state";
import type { Panel } from "../bindings";
import type { ContextMenuItem } from "../components/ContextMenu";

/**
 * UI contribution registry (PRD 07 §8 scaffold).
 *
 * Decouples the UI components that dispatch actions (ribbon, panel
 * toolbar, footer, status bar) from the code that implements them.
 * Today the registry is populated by `registerBuiltins()` at app boot.
 * When plugins come online they'll register through the same API —
 * either directly from JS (core plugins) or via an IPC bridge (WASM
 * community plugins).
 *
 * Unknown ids are non-fatal: invoking an unregistered command logs a
 * warning and no-ops. Presets referencing ids that aren't wired yet
 * therefore degrade gracefully instead of throwing.
 */

export type CommandHandler = (ctx?: unknown) => void | Promise<void>;
export type ViewOpener = (ctx?: unknown) => void | Promise<void>;

export interface ContentComponentProps {
  panel: Panel;
}
export type ContentComponent = ComponentType<ContentComponentProps>;

/**
 * Settings-modal tab contribution. The modal's left rail is grouped
 * into "options" (app-wide preferences) and "plugins" (the aggregate
 * Plugins tab + any plugin-contributed tabs). Built-ins register
 * through this same API so they're discoverable and orderable
 * alongside plugin additions — PRD-07 §20.
 */
export interface SettingsTab {
  /** Stable id, also used as the `activeTab` key. */
  id: string;
  title: string;
  /** Lucide-registry icon name shown in the rail. */
  icon: string;
  group: "options" | "plugins";
  /** Content-pane component. Rendered with no props. */
  component: ComponentType;
  /** Ordering hint within the group (lower first). Default 100. */
  order?: number;
}

/** Command-palette entry. References an already-registered command by
 *  `commandId`; the palette dispatches via `contributions.invokeCommand`. */
export interface PaletteCommand {
  /** Unique id for the palette entry (usually matches `commandId`). */
  id: string;
  /** Command id to dispatch when the user picks this entry. */
  commandId: string;
  /** Primary label shown in the palette list. */
  title: string;
  /** Optional category badge (e.g. "Workspace", "Editor"). */
  category?: string;
  /** Optional Lucide-registry icon name. */
  icon?: string;
  /** Optional keybinding hint shown right-aligned (e.g. "⌘K"). */
  keybinding?: string;
}

/**
 * Editor extension-point contributions (PRD-08 §14.1–14.3). The editor
 * surface (`EditorSurface.tsx`) subscribes to these lists and reconfigures
 * its CodeMirror instance whenever a plugin registers or disposes a
 * contribution, so hot-reloading a plugin picks up block types,
 * decorations, and editor-scoped keybindings without an app restart.
 */
export interface EditorBlockType {
  /** Stable id. Plugin contributions should be namespaced as `plugin:<id>:<block>`. */
  id: string;
  /** Display label for block-conversion menus and inserters. */
  label: string;
  /** Lucide-registry icon name. */
  icon: string;
  /** Short description shown as secondary text. */
  description?: string;
  /**
   * Serialize a block of this type to markdown. `content` is the
   * plain-text body; `attrs` is block-type-specific metadata.
   * Optional — metadata-only registrations are allowed for plugins
   * that only need the block to appear in menus.
   */
  toMarkdown?: (content: string, attrs?: Record<string, unknown>) => string;
}

export interface EditorDecorationProvider {
  /** Stable id for dedupe + disposal diagnostics. */
  id: string;
  /**
   * CodeMirror extension(s) the provider installs into the editor.
   * Providers typically return a `ViewPlugin` or `Decoration` source
   * but any valid CM6 extension is accepted so plugins can also
   * contribute gutter markers, tooltip hosts, etc. in the decoration
   * slot.
   */
  extension: Extension;
}

export interface EditorKeybinding {
  /** Stable id, namespaced for plugin entries. */
  id: string;
  /** CodeMirror keymap-style chord (e.g. "Mod-Shift-P", "Alt-Enter"). */
  key: string;
  /** Command id dispatched via `contributions.invokeCommand`. */
  commandId: string;
  /**
   * Reserved for `when` clause evaluation (e.g. "editorFocus",
   * "selection"). Not yet honored; editor keybindings are currently
   * always active while the CodeMirror surface has focus.
   */
  when?: string;
}

/**
 * Editor text-expansion snippet contributed via the registry (PRD-08 §14
 * extension). When the user types `trigger` and presses Tab, the editor
 * replaces the trigger text with `body`. Use `$CURSOR` in the body to
 * mark the final cursor position (omit for end-of-body placement).
 *
 * Plugin snippets are namespaced: id `"plugin:<pluginId>:<name>"`.
 */
export interface Snippet {
  /** Stable id. */
  id: string;
  /** Text the user types to activate the snippet (no spaces). */
  trigger: string;
  /**
   * Expansion text. `$CURSOR` is replaced by the final caret position.
   * If absent, the cursor lands at the end of the expanded body.
   */
  body: string;
  /** Short description shown in autocomplete hints. */
  description?: string;
  /**
   * Restrict the snippet to specific file extensions (lowercase, without dot).
   * Empty array or omitted = active in all file types.
   */
  fileTypes?: string[];
}

/**
 * A single node in a plugin-contributed tree view (PRD-07 §8 tree data provider).
 * `children` is omitted or undefined for leaf nodes, or null for unexpanded
 * branch nodes (lazy-load not yet triggered).
 */
export interface TreeNode {
  /** Stable, unique id within this provider's tree. */
  id: string;
  /** Display label. */
  label: string;
  /** Optional Lucide icon name. */
  icon?: string;
  /** Child nodes. `undefined` = leaf. `null` = unexpanded branch (lazy). */
  children?: TreeNode[] | null;
}

/**
 * Plugin tree-data provider. Plugins implement this interface and register
 * via `contributions.registerTreeDataProvider(viewId, provider)` to get a
 * free generic tree panel rendered by the host shell. No bespoke React
 * component required — the host drives rendering via `getChildren`.
 */
export interface TreeDataProvider {
  /** Stable id matching `viewId`. Used for diagnostics. */
  id: string;
  /**
   * Fetch child nodes. Pass `null` to get the root-level items.
   * May return synchronously or as a Promise (for async data sources).
   */
  getChildren(nodeId: string | null): TreeNode[] | Promise<TreeNode[]>;
  /** Called when the user clicks a node. Optional. */
  onSelect?(nodeId: string, node: TreeNode): void | Promise<void>;
}

/**
 * Plugin- or host-contributed menu-bar item (PRD-07 §7.5).
 *
 * The `menu` field names the top-level pull-down: `"File"`, `"Edit"`,
 * `"View"`, `"Help"`, or any plugin-defined label. Submenu nesting is
 * reserved for the future via `"File > New"` path syntax; today only the
 * first path segment is rendered. Actions dispatch via `invokeCommand` so
 * the command registry is the single authority.
 */
export interface MenuItem {
  /** Stable id. Plugin items namespaced: `"plugin:<pluginId>:<action>"`. */
  id: string;
  /** User-visible label for this entry within its pull-down menu. */
  label: string;
  /** Command id dispatched via `contributions.invokeCommand` when selected. */
  commandId: string;
  /**
   * Top-level menu label this item belongs to.
   * e.g. `"File"`, `"Edit"`, `"View"`, `"Help"`.
   * Submenu paths (`"File > New"`) are reserved — only the first segment
   * is used today.
   */
  menu: string;
  /** Render a separator above this item within the pull-down. */
  separatorBefore?: boolean;
  /** Dim the item (action still dispatches unless the command is a no-op). */
  disabled?: boolean;
  /** Optional Lucide icon name. Reserved for future icon support. */
  icon?: string;
  /**
   * Ordering hint within the pull-down (lower first). Default 100.
   * Top-level menu ordering is alphabetical by label by default; use
   * `menuOrder` on an item to influence which top-level menus appear first.
   */
  order?: number;
  /**
   * Ordering hint for the top-level menu label itself (lower appears left).
   * All items sharing the same `menu` label use the minimum `menuOrder`
   * found among them. Default 100.
   */
  menuOrder?: number;
}

/**
 * Plugin-contributed context menu item. Action dispatches via the command
 * registry so plugins don't hold raw function references in the registry.
 *
 * Scope strings follow the convention `"surface:kind"`, e.g.:
 *   - `"file-tree:file"`      — right-click on a file row
 *   - `"file-tree:directory"` — right-click on a folder row
 *   - `"file-tree:root"`      — right-click on the empty area
 *   - `"tab"`                 — right-click on a tab label (future)
 *   - `"editor:selection"`    — right-click inside the editor (future)
 */
export interface ContribContextMenuItem {
  /** Stable id. Namespaced for plugin items: `"plugin:<pluginId>:<action>"`. */
  id: string;
  /** User-visible label. */
  label: string;
  /** Command id dispatched via `contributions.invokeCommand` when selected. */
  commandId: string;
  /** Scope(s) this item applies to. A single item can appear in multiple scopes. */
  scopes: string[];
  /** Render a separator above this item in the menu. */
  separatorBefore?: boolean;
  /** Dim the item (action still dispatches unless the command is a no-op). */
  disabled?: boolean;
  /** Optional Lucide icon name. Not rendered yet; reserved for future icon support. */
  icon?: string;
}

type Disposable = () => void;

const commands = new Map<string, CommandHandler>();
const views = new Map<string, ViewOpener>();
const contentTypes = new Map<string, ContentComponent>();
const paletteCommands = new Map<string, PaletteCommand>();
const settingsTabs = new Map<string, SettingsTab>();
const editorBlockTypes = new Map<string, EditorBlockType>();
const editorDecorationProviders = new Map<string, EditorDecorationProvider>();
const editorKeybindings = new Map<string, EditorKeybinding>();
const treeDataProviders = new Map<string, TreeDataProvider>();
const treeDataProviderListeners = new Set<() => void>();
const snippets = new Map<string, Snippet>();
const snippetListeners = new Set<() => void>();

/**
 * File extension → content-type id map. Keys are lowercase extensions
 * without the leading dot (e.g. `"md"`, `"canvas"`). Plugins register
 * via `contributions.registerFileHandler` so opening a file in the forge
 * picks the correct tab surface instead of falling through to FileViewer.
 */
const fileHandlers = new Map<string, string>();
const fileHandlerListeners = new Set<() => void>();

/**
 * Flat list of all menu-bar items. Grouped and sorted at render time.
 */
const menuItems = new Map<string, MenuItem>();
const menuItemListeners = new Set<() => void>();

/**
 * Scope-keyed list of plugin-contributed context menu items. Stored as a
 * flat list and filtered by scope at render time so one item can appear
 * in multiple scopes without duplication in the data layer.
 */
const contextMenuItems = new Map<string, ContribContextMenuItem>();
const contextMenuListeners = new Set<() => void>();

/**
 * Factory function injected at boot (by `registerBuiltins`) to create the
 * GenericTreePanel content component for a given view id. This avoids a
 * circular ESM import between registry.ts → GenericTreePanel → registry.ts.
 */
let _treePanelFactory: ((viewId: string) => ContentComponent) | null = null;
/**
 * User keybinding overrides, keyed by `commandId`. When present, the
 * effective keybinding exposed via `listPaletteCommands` / the React
 * hook is the override, not the manifest-declared default.
 * Hydrated from Rust on boot via `hydrateKeybindingOverrides`.
 */
const keybindingOverrides = new Map<string, string>();
const contentTypeListeners = new Set<() => void>();
const paletteListeners = new Set<() => void>();
const settingsTabListeners = new Set<() => void>();
const editorBlockTypeListeners = new Set<() => void>();
const editorDecorationListeners = new Set<() => void>();
const editorKeybindingListeners = new Set<() => void>();
// fileHandlerListeners and contextMenuListeners declared above with their Maps

function warn(msg: string) {
  // eslint-disable-next-line no-console
  console.warn(`[contributions] ${msg}`);
}

function safeInvoke(label: string, fn: () => unknown) {
  try {
    const result = fn();
    if (result instanceof Promise) {
      result.catch((err) => warn(`${label} rejected: ${String(err)}`));
    }
  } catch (err) {
    warn(`${label} threw: ${String(err)}`);
  }
}

export const contributions = {
  registerCommand(id: string, handler: CommandHandler): Disposable {
    if (commands.has(id)) warn(`command '${id}' already registered — replacing`);
    commands.set(id, handler);
    return () => {
      if (commands.get(id) === handler) commands.delete(id);
    };
  },

  registerView(id: string, opener: ViewOpener): Disposable {
    if (views.has(id)) warn(`view '${id}' already registered — replacing`);
    views.set(id, opener);
    return () => {
      if (views.get(id) === opener) views.delete(id);
    };
  },

  registerContentType(id: string, component: ContentComponent): Disposable {
    if (contentTypes.has(id)) warn(`contentType '${id}' already registered — replacing`);
    contentTypes.set(id, component);
    contentTypeListeners.forEach((fn) => fn());
    return () => {
      if (contentTypes.get(id) === component) {
        contentTypes.delete(id);
        contentTypeListeners.forEach((fn) => fn());
      }
    };
  },

  invokeCommand(id: string, ctx?: unknown): void {
    const handler = commands.get(id);
    if (!handler) {
      warn(`invokeCommand: no handler for '${id}'`);
      return;
    }
    safeInvoke(`command '${id}'`, () => handler(ctx));
  },

  openView(id: string, ctx?: unknown): void {
    const opener = views.get(id);
    if (!opener) {
      warn(`openView: no opener for '${id}'`);
      return;
    }
    safeInvoke(`view '${id}'`, () => opener(ctx));
  },

  resolveContentType(id: string): ContentComponent | undefined {
    return contentTypes.get(id);
  },

  hasCommand(id: string): boolean {
    return commands.has(id);
  },

  hasView(id: string): boolean {
    return views.has(id);
  },

  /** Subscribe to content-type registration changes. Used by PanelView
   *  so a late-registered renderer swaps in without a full reload. */
  subscribeContentTypes(fn: () => void): Disposable {
    contentTypeListeners.add(fn);
    return () => {
      contentTypeListeners.delete(fn);
    };
  },

  registerPaletteCommand(item: PaletteCommand): Disposable {
    if (paletteCommands.has(item.id)) {
      warn(`palette command '${item.id}' already registered — replacing`);
    }
    paletteCommands.set(item.id, item);
    paletteSnapshot = null;
    paletteListeners.forEach((fn) => fn());
    return () => {
      if (paletteCommands.get(item.id) === item) {
        paletteCommands.delete(item.id);
        paletteSnapshot = null;
        paletteListeners.forEach((fn) => fn());
      }
    };
  },

  listPaletteCommands(): PaletteCommand[] {
    return Array.from(paletteCommands.values()).map(applyOverride);
  },

  /**
   * Set the keybinding override for `commandId`. In-memory only — the
   * caller is responsible for persisting via the `set_keybinding_override`
   * Tauri command. Fires palette listeners so the dispatcher + Hotkeys
   * tab re-render immediately.
   */
  setKeybindingOverride(commandId: string, binding: string): void {
    keybindingOverrides.set(commandId, binding);
    paletteSnapshot = null;
    paletteListeners.forEach((fn) => fn());
  },

  /** Remove the override for `commandId`, reverting to the manifest default. */
  clearKeybindingOverride(commandId: string): void {
    if (!keybindingOverrides.delete(commandId)) return;
    paletteSnapshot = null;
    paletteListeners.forEach((fn) => fn());
  },

  /** Return the raw override string for `commandId`, or `undefined`. */
  getKeybindingOverride(commandId: string): string | undefined {
    return keybindingOverrides.get(commandId);
  },

  /**
   * Replace the entire override map. Used once at boot to hydrate from
   * the persisted file before the dispatcher mounts.
   */
  hydrateKeybindingOverrides(overrides: Record<string, string>): void {
    keybindingOverrides.clear();
    for (const [id, binding] of Object.entries(overrides)) {
      if (binding) keybindingOverrides.set(id, binding);
    }
    paletteSnapshot = null;
    paletteListeners.forEach((fn) => fn());
  },

  subscribePaletteCommands(fn: () => void): Disposable {
    paletteListeners.add(fn);
    return () => {
      paletteListeners.delete(fn);
    };
  },

  registerSettingsTab(tab: SettingsTab): Disposable {
    if (settingsTabs.has(tab.id)) {
      warn(`settings tab '${tab.id}' already registered — replacing`);
    }
    settingsTabs.set(tab.id, tab);
    settingsTabsSnapshot = null;
    settingsTabListeners.forEach((fn) => fn());
    return () => {
      if (settingsTabs.get(tab.id) === tab) {
        settingsTabs.delete(tab.id);
        settingsTabsSnapshot = null;
        settingsTabListeners.forEach((fn) => fn());
      }
    };
  },

  resolveSettingsTab(id: string): SettingsTab | undefined {
    return settingsTabs.get(id);
  },

  listSettingsTabs(): SettingsTab[] {
    return settingsTabsSnapshotFn();
  },

  subscribeSettingsTabs(fn: () => void): Disposable {
    settingsTabListeners.add(fn);
    return () => {
      settingsTabListeners.delete(fn);
    };
  },

  registerEditorBlockType(type: EditorBlockType): Disposable {
    if (editorBlockTypes.has(type.id)) {
      warn(`editor block type '${type.id}' already registered — replacing`);
    }
    editorBlockTypes.set(type.id, type);
    editorBlockTypesSnapshot = null;
    editorBlockTypeListeners.forEach((fn) => fn());
    return () => {
      if (editorBlockTypes.get(type.id) === type) {
        editorBlockTypes.delete(type.id);
        editorBlockTypesSnapshot = null;
        editorBlockTypeListeners.forEach((fn) => fn());
      }
    };
  },

  listEditorBlockTypes(): EditorBlockType[] {
    return editorBlockTypesSnapshotFn();
  },

  subscribeEditorBlockTypes(fn: () => void): Disposable {
    editorBlockTypeListeners.add(fn);
    return () => {
      editorBlockTypeListeners.delete(fn);
    };
  },

  registerEditorDecorationProvider(provider: EditorDecorationProvider): Disposable {
    if (editorDecorationProviders.has(provider.id)) {
      warn(`editor decoration provider '${provider.id}' already registered — replacing`);
    }
    editorDecorationProviders.set(provider.id, provider);
    editorDecorationSnapshot = null;
    editorDecorationListeners.forEach((fn) => fn());
    return () => {
      if (editorDecorationProviders.get(provider.id) === provider) {
        editorDecorationProviders.delete(provider.id);
        editorDecorationSnapshot = null;
        editorDecorationListeners.forEach((fn) => fn());
      }
    };
  },

  listEditorDecorationProviders(): EditorDecorationProvider[] {
    return editorDecorationSnapshotFn();
  },

  subscribeEditorDecorationProviders(fn: () => void): Disposable {
    editorDecorationListeners.add(fn);
    return () => {
      editorDecorationListeners.delete(fn);
    };
  },

  registerEditorKeybinding(binding: EditorKeybinding): Disposable {
    if (editorKeybindings.has(binding.id)) {
      warn(`editor keybinding '${binding.id}' already registered — replacing`);
    }
    editorKeybindings.set(binding.id, binding);
    editorKeybindingSnapshot = null;
    editorKeybindingListeners.forEach((fn) => fn());
    return () => {
      if (editorKeybindings.get(binding.id) === binding) {
        editorKeybindings.delete(binding.id);
        editorKeybindingSnapshot = null;
        editorKeybindingListeners.forEach((fn) => fn());
      }
    };
  },

  listEditorKeybindings(): EditorKeybinding[] {
    return editorKeybindingSnapshotFn();
  },

  subscribeEditorKeybindings(fn: () => void): Disposable {
    editorKeybindingListeners.add(fn);
    return () => {
      editorKeybindingListeners.delete(fn);
    };
  },

  /**
   * Inject the GenericTreePanel factory. Called once at app boot from
   * `registerBuiltins` after `GenericTreePanel` is imported, so this
   * file stays free of a direct import (circular-dep avoidance).
   */
  setTreePanelFactory(factory: (viewId: string) => ContentComponent): void {
    _treePanelFactory = factory;
  },

  /**
   * Register a tree-data provider for `viewId` and auto-wire a
   * GenericTreePanel content-type component so plugins don't need to
   * ship bespoke React code for simple tree views.
   *
   * Requires `setTreePanelFactory` to have been called at boot.
   * Returns a disposable that removes both the provider and content-type
   * registration when the plugin stops.
   */
  registerTreeDataProvider(viewId: string, provider: TreeDataProvider): Disposable {
    if (treeDataProviders.has(viewId)) {
      warn(`tree data provider '${viewId}' already registered — replacing`);
    }
    treeDataProviders.set(viewId, provider);
    treeDataProviderListeners.forEach((fn) => fn());

    let disposeContent: Disposable | undefined;
    if (_treePanelFactory) {
      disposeContent = contributions.registerContentType(viewId, _treePanelFactory(viewId));
    } else {
      warn(`registerTreeDataProvider('${viewId}'): tree panel factory not set — call setTreePanelFactory at boot`);
    }

    return () => {
      if (treeDataProviders.get(viewId) === provider) {
        treeDataProviders.delete(viewId);
        treeDataProviderListeners.forEach((fn) => fn());
      }
      disposeContent?.();
    };
  },

  resolveTreeDataProvider(viewId: string): TreeDataProvider | undefined {
    return treeDataProviders.get(viewId);
  },

  subscribeTreeDataProviders(fn: () => void): Disposable {
    treeDataProviderListeners.add(fn);
    return () => {
      treeDataProviderListeners.delete(fn);
    };
  },

  /**
   * Map a file extension (without leading dot, case-insensitive) to a
   * registered content-type id. When a file is opened the shell looks up
   * the extension here and, if a match is found, opens a tab with that
   * `contentType` instead of the generic FileViewer fallback.
   *
   * Example: `registerFileHandler("canvas", "com.nexus.canvas.editor")`
   */
  registerFileHandler(ext: string, contentTypeId: string): Disposable {
    const key = ext.toLowerCase().replace(/^\./, "");
    if (fileHandlers.has(key)) {
      warn(`file handler for '.${key}' already registered — replacing`);
    }
    fileHandlers.set(key, contentTypeId);
    fileHandlerListeners.forEach((fn) => fn());
    return () => {
      if (fileHandlers.get(key) === contentTypeId) {
        fileHandlers.delete(key);
        fileHandlerListeners.forEach((fn) => fn());
      }
    };
  },

  /**
   * Resolve the content-type id for a file extension, or `undefined` if
   * no handler is registered. `ext` may include or omit the leading dot.
   */
  resolveFileHandler(ext: string): string | undefined {
    const key = ext.toLowerCase().replace(/^\./, "");
    return fileHandlers.get(key);
  },

  /**
   * Resolve the content-type id for an absolute or relative file path by
   * extracting its extension and forwarding to `resolveFileHandler`.
   * Returns `undefined` if no handler is registered for the extension.
   */
  resolveFileHandlerForPath(filePath: string): string | undefined {
    const dot = filePath.lastIndexOf(".");
    const slash = Math.max(filePath.lastIndexOf("/"), filePath.lastIndexOf("\\"));
    if (dot <= slash || dot === -1) return undefined;
    return this.resolveFileHandler(filePath.slice(dot + 1));
  },

  subscribeFileHandlers(fn: () => void): Disposable {
    fileHandlerListeners.add(fn);
    return () => {
      fileHandlerListeners.delete(fn);
    };
  },

  /**
   * Register a menu-bar item (PRD-07 §7.5). The item appears under the
   * pull-down named by `item.menu` and its action dispatches via
   * `contributions.invokeCommand(item.commandId)` when selected.
   *
   * Returns a disposable that removes the item when called — essential
   * for plugin hot-reload: call this disposable from the plugin's `onStop`.
   */
  registerMenuItem(item: MenuItem): Disposable {
    if (menuItems.has(item.id)) {
      warn(`menu item '${item.id}' already registered — replacing`);
    }
    menuItems.set(item.id, item);
    menuItemsSnapshot = null;
    menuItemListeners.forEach((fn) => fn());
    return () => {
      if (menuItems.get(item.id) === item) {
        menuItems.delete(item.id);
        menuItemsSnapshot = null;
        menuItemListeners.forEach((fn) => fn());
      }
    };
  },

  listMenuItems(): MenuItem[] {
    return menuItemsSnapshotFn();
  },

  subscribeMenuItems(fn: () => void): Disposable {
    menuItemListeners.add(fn);
    return () => {
      menuItemListeners.delete(fn);
    };
  },

  /**
   * Register a plugin-contributed context menu item for one or more scopes
   * (e.g. `"file-tree:file"`, `"file-tree:directory"`). The item's action
   * dispatches via `contributions.invokeCommand(item.commandId)` so the
   * command registry is the single authority over what runs.
   *
   * Items registered for the same scope appear after the built-in items in
   * the menu, separated by the first item's `separatorBefore: true`.
   */
  registerContextMenuItem(item: ContribContextMenuItem): Disposable {
    if (contextMenuItems.has(item.id)) {
      warn(`context menu item '${item.id}' already registered — replacing`);
    }
    contextMenuItems.set(item.id, item);
    contextMenuSnapshot.clear();
    contextMenuListeners.forEach((fn) => fn());
    return () => {
      if (contextMenuItems.get(item.id) === item) {
        contextMenuItems.delete(item.id);
        contextMenuSnapshot.clear();
        contextMenuListeners.forEach((fn) => fn());
      }
    };
  },

  /**
   * Return all registered context menu items for the given scope, converted
   * to `ContextMenuItem` records ready for the `<ContextMenu>` component.
   * Items are sorted by registration order (stable across re-renders via
   * Map insertion order).
   */
  listContextMenuItems(scope: string): ContextMenuItem[] {
    return Array.from(contextMenuItems.values())
      .filter((item) => item.scopes.includes(scope))
      .map((item) => ({
        id: item.id,
        label: item.label,
        onSelect: () => contributions.invokeCommand(item.commandId),
        separatorBefore: item.separatorBefore,
        disabled: item.disabled,
      }));
  },

  subscribeContextMenuItems(fn: () => void): Disposable {
    contextMenuListeners.add(fn);
    return () => {
      contextMenuListeners.delete(fn);
    };
  },

  /**
   * Register a text-expansion snippet. When the user types `trigger` and
   * presses Tab in the editor, the trigger is replaced with `body`.
   *
   * Returns a disposable that removes the snippet when the plugin stops.
   */
  registerSnippet(snippet: Snippet): Disposable {
    if (snippets.has(snippet.id)) {
      warn(`snippet '${snippet.id}' already registered — replacing`);
    }
    snippets.set(snippet.id, snippet);
    snippetSnapshot = null;
    snippetListeners.forEach((fn) => fn());
    return () => {
      if (snippets.get(snippet.id) === snippet) {
        snippets.delete(snippet.id);
        snippetSnapshot = null;
        snippetListeners.forEach((fn) => fn());
      }
    };
  },

  listSnippets(): Snippet[] {
    return snippetSnapshotFn();
  },

  subscribeSnippets(fn: () => void): Disposable {
    snippetListeners.add(fn);
    return () => {
      snippetListeners.delete(fn);
    };
  },
};

/** Reset all registrations. Test-only. */
export function __resetContributions() {
  commands.clear();
  views.clear();
  contentTypes.clear();
  paletteCommands.clear();
  settingsTabs.clear();
  editorBlockTypes.clear();
  editorDecorationProviders.clear();
  editorKeybindings.clear();
  keybindingOverrides.clear();
  treeDataProviders.clear();
  fileHandlers.clear();
  menuItems.clear();
  menuItemListeners.clear();
  contextMenuItems.clear();
  snippets.clear();
  contentTypeListeners.clear();
  paletteListeners.clear();
  settingsTabListeners.clear();
  editorBlockTypeListeners.clear();
  editorDecorationListeners.clear();
  editorKeybindingListeners.clear();
  treeDataProviderListeners.clear();
  fileHandlerListeners.clear();
  contextMenuListeners.clear();
  snippetListeners.clear();
  paletteSnapshot = null;
  settingsTabsSnapshot = null;
  editorBlockTypesSnapshot = null;
  editorDecorationSnapshot = null;
  editorKeybindingSnapshot = null;
  snippetSnapshot = null;
  menuItemsSnapshot = null;
  contextMenuSnapshot.clear();
}

/**
 * Return `cmd` with its `keybinding` replaced by the user override if
 * one exists. The stored `PaletteCommand` keeps the manifest-declared
 * default untouched; this merge is what consumers (palette, Hotkeys
 * tab, keybinding dispatcher) actually see.
 */
function applyOverride(cmd: PaletteCommand): PaletteCommand {
  const override = keybindingOverrides.get(cmd.commandId);
  if (override === undefined) return cmd;
  return { ...cmd, keybinding: override };
}

/**
 * React hook returning the content-type component for `id`, or
 * `undefined` if no component is registered. Re-renders when the
 * registration set changes (e.g. a plugin registers after mount).
 */
export function useContentType(id: string | null | undefined): ContentComponent | undefined {
  return useSyncExternalStore(
    (notify) => contributions.subscribeContentTypes(notify),
    () => (id ? contentTypes.get(id) : undefined),
    () => (id ? contentTypes.get(id) : undefined),
  );
}

/**
 * Cached snapshot of the palette command list. `useSyncExternalStore`
 * demands stable identity across reads with no state change;
 * `registerPaletteCommand` invalidates this synchronously before
 * firing listeners so React sees a fresh array on the commit-time
 * snapshot check.
 */
let paletteSnapshot: PaletteCommand[] | null = null;

function paletteSnapshotFn(): PaletteCommand[] {
  if (!paletteSnapshot) {
    paletteSnapshot = Array.from(paletteCommands.values()).map(applyOverride);
  }
  return paletteSnapshot;
}

/** React hook returning all registered palette commands, reactive to
 *  registrations/unregistrations. */
export function usePaletteCommands(): PaletteCommand[] {
  return useSyncExternalStore(
    (notify) => contributions.subscribePaletteCommands(notify),
    paletteSnapshotFn,
    paletteSnapshotFn,
  );
}

/**
 * Cached snapshot of the settings-tab list. Sorted by `order` (lower
 * first; default 100) with registration order as the stable tiebreaker
 * so the rail layout stays deterministic as plugins come and go.
 */
let settingsTabsSnapshot: SettingsTab[] | null = null;

function settingsTabsSnapshotFn(): SettingsTab[] {
  if (!settingsTabsSnapshot) {
    const entries = Array.from(settingsTabs.values());
    settingsTabsSnapshot = entries.sort(
      (a, b) => (a.order ?? 100) - (b.order ?? 100),
    );
  }
  return settingsTabsSnapshot;
}

/** React hook returning all registered settings tabs, reactive to
 *  registrations/unregistrations. */
export function useSettingsTabs(): SettingsTab[] {
  return useSyncExternalStore(
    (notify) => contributions.subscribeSettingsTabs(notify),
    settingsTabsSnapshotFn,
    settingsTabsSnapshotFn,
  );
}

/** Cached snapshot of the editor block-type list. Insertion order. */
let editorBlockTypesSnapshot: EditorBlockType[] | null = null;

function editorBlockTypesSnapshotFn(): EditorBlockType[] {
  if (!editorBlockTypesSnapshot) {
    editorBlockTypesSnapshot = Array.from(editorBlockTypes.values());
  }
  return editorBlockTypesSnapshot;
}

/** React hook returning all registered editor block types. */
export function useEditorBlockTypes(): EditorBlockType[] {
  return useSyncExternalStore(
    (notify) => contributions.subscribeEditorBlockTypes(notify),
    editorBlockTypesSnapshotFn,
    editorBlockTypesSnapshotFn,
  );
}

/** Cached snapshot of the editor decoration-provider list. */
let editorDecorationSnapshot: EditorDecorationProvider[] | null = null;

function editorDecorationSnapshotFn(): EditorDecorationProvider[] {
  if (!editorDecorationSnapshot) {
    editorDecorationSnapshot = Array.from(editorDecorationProviders.values());
  }
  return editorDecorationSnapshot;
}

/** Cached snapshot of the editor keybinding list. */
let editorKeybindingSnapshot: EditorKeybinding[] | null = null;

function editorKeybindingSnapshotFn(): EditorKeybinding[] {
  if (!editorKeybindingSnapshot) {
    editorKeybindingSnapshot = Array.from(editorKeybindings.values());
  }
  return editorKeybindingSnapshot;
}

/**
 * React hook returning the tree-data provider registered for `viewId`, or
 * `undefined` if none is registered. Re-renders when a provider is
 * added or removed (e.g. on plugin hot-reload).
 */
export function useTreeDataProvider(viewId: string): TreeDataProvider | undefined {
  return useSyncExternalStore(
    (notify) => contributions.subscribeTreeDataProviders(notify),
    () => treeDataProviders.get(viewId),
    () => treeDataProviders.get(viewId),
  );
}

/**
 * Per-scope snapshot cache for context menu items. Invalidated (cleared) by
 * `registerContextMenuItem` / disposable so useSyncExternalStore sees a stable
 * reference between renders and only re-renders when the registry mutates.
 * Context menus open infrequently, so O(items) filtering on invalidation is fine.
 */
const contextMenuSnapshot = new Map<string, ContextMenuItem[]>();

/**
 * React hook returning plugin-contributed context menu items for `scope`.
 * Re-renders when contributions change (plugin hot-reload, registration).
 */
export function useContextMenuItems(scope: string): ContextMenuItem[] {
  return useSyncExternalStore(
    (notify) => contributions.subscribeContextMenuItems(notify),
    () => {
      let snap = contextMenuSnapshot.get(scope);
      if (!snap) {
        snap = contributions.listContextMenuItems(scope);
        contextMenuSnapshot.set(scope, snap);
      }
      return snap;
    },
    () => contributions.listContextMenuItems(scope),
  );
}

/**
 * React hook returning the content-type id for a file extension, or
 * `undefined` if no handler is registered. Reactive: re-renders when
 * plugins register or remove file handlers.
 */
export function useFileHandler(ext: string | null | undefined): string | undefined {
  return useSyncExternalStore(
    (notify) => contributions.subscribeFileHandlers(notify),
    () => (ext ? contributions.resolveFileHandler(ext) : undefined),
    () => (ext ? contributions.resolveFileHandler(ext) : undefined),
  );
}

/** Cached snapshot of the snippet list. Insertion order. */
let snippetSnapshot: Snippet[] | null = null;

function snippetSnapshotFn(): Snippet[] {
  if (!snippetSnapshot) {
    snippetSnapshot = Array.from(snippets.values());
  }
  return snippetSnapshot;
}

/**
 * React hook returning all registered snippets. Re-renders when a snippet
 * is added or removed. Consumed by the snippet CM6 extension to stay in
 * sync with plugin hot-reloads.
 */
export function useSnippets(): Snippet[] {
  return useSyncExternalStore(
    (notify) => contributions.subscribeSnippets(notify),
    snippetSnapshotFn,
    snippetSnapshotFn,
  );
}

/**
 * Cached flat snapshot of menu items sorted by menu label order then item order.
 * Invalidated on any `registerMenuItem` / disposal.
 */
let menuItemsSnapshot: MenuItem[] | null = null;

function menuItemsSnapshotFn(): MenuItem[] {
  if (!menuItemsSnapshot) {
    const entries = Array.from(menuItems.values());
    menuItemsSnapshot = entries.sort((a, b) => {
      const moA = a.menuOrder ?? 100;
      const moB = b.menuOrder ?? 100;
      if (moA !== moB) return moA - moB;
      const labelCmp = a.menu.localeCompare(b.menu);
      if (labelCmp !== 0) return labelCmp;
      return (a.order ?? 100) - (b.order ?? 100);
    });
  }
  return menuItemsSnapshot;
}

/**
 * React hook returning all registered menu-bar items, sorted by menu order
 * then item order. Re-renders when items are added or removed (plugin
 * hot-reload safe).
 */
export function useMenuItems(): MenuItem[] {
  return useSyncExternalStore(
    (notify) => contributions.subscribeMenuItems(notify),
    menuItemsSnapshotFn,
    menuItemsSnapshotFn,
  );
}
