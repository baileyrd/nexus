import { useSyncExternalStore, type ComponentType } from "react";
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

type Disposable = () => void;

const commands = new Map<string, CommandHandler>();
const views = new Map<string, ViewOpener>();
const contentTypes = new Map<string, ContentComponent>();
const paletteCommands = new Map<string, PaletteCommand>();
const contentTypeListeners = new Set<() => void>();
const paletteListeners = new Set<() => void>();

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
    return Array.from(paletteCommands.values());
  },

  subscribePaletteCommands(fn: () => void): Disposable {
    paletteListeners.add(fn);
    return () => {
      paletteListeners.delete(fn);
    };
  },
};

/** Reset all registrations. Test-only. */
export function __resetContributions() {
  commands.clear();
  views.clear();
  contentTypes.clear();
  paletteCommands.clear();
  contentTypeListeners.clear();
  paletteListeners.clear();
  paletteSnapshot = null;
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
    paletteSnapshot = Array.from(paletteCommands.values());
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
