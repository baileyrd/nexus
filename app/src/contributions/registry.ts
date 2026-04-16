import { useSyncExternalStore, type ComponentType } from "react";
import type { Extension } from "@codemirror/state";
import type { Panel } from "../bindings";

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

type Disposable = () => void;

const commands = new Map<string, CommandHandler>();
const views = new Map<string, ViewOpener>();
const contentTypes = new Map<string, ContentComponent>();
const paletteCommands = new Map<string, PaletteCommand>();
const settingsTabs = new Map<string, SettingsTab>();
const editorBlockTypes = new Map<string, EditorBlockType>();
const editorDecorationProviders = new Map<string, EditorDecorationProvider>();
const editorKeybindings = new Map<string, EditorKeybinding>();
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
  contentTypeListeners.clear();
  paletteListeners.clear();
  settingsTabListeners.clear();
  editorBlockTypeListeners.clear();
  editorDecorationListeners.clear();
  editorKeybindingListeners.clear();
  paletteSnapshot = null;
  settingsTabsSnapshot = null;
  editorBlockTypesSnapshot = null;
  editorDecorationSnapshot = null;
  editorKeybindingSnapshot = null;
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
