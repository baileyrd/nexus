/**
 * Public TypeScript surface for Nexus script-plugin authors (UI F-2.1.1).
 *
 * ## Versioning
 *
 * This package ships with semver discipline. The `0.x` line mirrors
 * whatever the shell happens to expose today; a `1.0.0` tag signals
 * that every exported shape is frozen and future `1.x` releases add
 * surface but never change existing signatures.
 *
 * ## Runtime
 *
 * This package is **types-only**. Plugins import the shapes from here
 * so TypeScript catches accidental contract drift when the shell is
 * updated, but the `NexusPluginContext` instance passed to
 * `dispatch` / `onInit` / `onStart` / `onStop` at runtime is still
 * supplied by the Nexus host. There is nothing to `new` from this
 * package.
 *
 * ## What's in scope
 *
 * - Every field on `NexusPluginContext`, including subsurface types
 *   (`Disposable`, `DisposableStore`, `DeclaredCapabilities`).
 * - Every contribution DTO the host consumes
 *   (`EditorBlockType`, `EditorDecorationProvider`, `EditorKeybinding`,
 *   `Snippet`, `MenuItem`, `ContextMenuItem`, `UriHandler`,
 *   `WebviewPanelConfig`, `TreeDataProvider`, `TreeNode`, `PanelNode`).
 * - `ScriptPlugin` — the shape a plugin's default export must satisfy.
 *
 * ## What's out of scope
 *
 * - React components. This package has no `react` dependency so plugins
 *   that ship JSX must depend on React themselves.
 * - CM6 extension types. Plugins that contribute editor decorations
 *   still depend on `@codemirror/*` directly — we re-export only the
 *   host-facing `Extension` opaque reference.
 */

export type ToastLevel = "info" | "warn" | "error";

/** Disposal callback — returned from every `register*` API. */
export type Disposable = () => void;

/**
 * Collects disposables and flushes them in LIFO order on plugin stop.
 * The host auto-creates one per plugin and attaches it as
 * `ctx.disposables`; plugins simply call `add(d)` on every register.
 */
export interface DisposableStore {
  add(d: Disposable): Disposable;
  dispose(): void;
  readonly size: number;
}

/** Optional capability-set used by the host to gate context surfaces. */
export type DeclaredCapabilities = ReadonlySet<string> | undefined;

// ─── Editor contributions ────────────────────────────────────────────────────

export interface EditorBlockType {
  id: string;
  label: string;
  icon: string;
  description?: string;
  toMarkdown?: (content: string, attrs?: Record<string, unknown>) => string;
}

/**
 * Opaque CodeMirror 6 extension. Plugins that contribute decorations
 * should import the concrete `Extension` type from `@codemirror/state`
 * for better type inference; this alias exists so the extension-api
 * package stays free of a CM6 peer dependency.
 */
export type EditorExtension = unknown;

export interface EditorDecorationProvider {
  id: string;
  extension: EditorExtension;
}

export interface EditorKeybinding {
  id: string;
  key: string;
  commandId: string;
}

export interface Snippet {
  id: string;
  trigger: string;
  body: string;
  description?: string;
  fileTypes?: string[];
}

// ─── UI contributions ────────────────────────────────────────────────────────

export interface TreeNode {
  id: string;
  label: string;
  icon?: string;
  children?: TreeNode[] | null;
}

export interface TreeDataProvider {
  id: string;
  getChildren(nodeId: string | null): TreeNode[] | Promise<TreeNode[]>;
  onSelect?(nodeId: string, node: TreeNode): void | Promise<void>;
}

export interface WebviewPanelConfig {
  htmlUrl: string;
  allowPopups?: boolean;
}

export interface UriHandler {
  id: string;
  scheme: string;
  handle(url: string): void | Promise<void>;
}

export interface MenuItem {
  id: string;
  label: string;
  commandId: string;
  menu: string;
  separatorBefore?: boolean;
  disabled?: boolean;
  icon?: string;
  order?: number;
  menuOrder?: number;
}

export interface ContextMenuItem {
  id: string;
  label: string;
  commandId: string;
  scopes: string[];
  separatorBefore?: boolean;
  disabled?: boolean;
  icon?: string;
}

// ─── Declarative panel primitives (UI F-5.2.1) ───────────────────────────────

export type PanelNode =
  | { type: "vstack"; gap?: number; children: PanelNode[] }
  | { type: "hstack"; gap?: number; children: PanelNode[] }
  | { type: "text"; value: string; muted?: boolean; strong?: boolean }
  | { type: "heading"; value: string; level?: 1 | 2 | 3 }
  | { type: "button"; label: string; commandId: string; disabled?: boolean }
  | { type: "spacer"; size?: number };

export type PanelRenderFn = () => PanelNode;

// ─── Host-provided context ───────────────────────────────────────────────────

export interface NexusPluginContext {
  pluginId: string;

  settings: {
    get(): Promise<Record<string, unknown>>;
  };

  events: {
    emit(typeId: string, payload: unknown): Promise<void>;
  };

  ipc: {
    call(
      targetPluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<unknown>;
  };

  editor: {
    registerBlockType(type: EditorBlockType): Disposable;
    registerDecorationProvider(provider: EditorDecorationProvider): Disposable;
    registerKeybinding(binding: EditorKeybinding): Disposable;
    registerSnippet(snippet: Snippet): Disposable;
  };

  ui: {
    notify(level: ToastLevel, message: string): void;
    registerTreeDataProvider(
      viewId: string,
      provider: TreeDataProvider,
    ): Disposable;
    registerFileHandler(ext: string, contentTypeId: string): Disposable;
    registerContextMenuItem(item: ContextMenuItem): Disposable;
    registerMenuItem(item: MenuItem): Disposable;
    registerUriHandler(handler: UriHandler): Disposable;
    registerWebviewPanel(viewId: string, config: WebviewPanelConfig): Disposable;
    registerPanelView(viewId: string, render: PanelRenderFn): Disposable;
  };

  workspace: {
    root(): string | null;
    name(): string | null;
  };

  editorActive: {
    relpath(): string | null;
    content(): string | null;
    isDirty(): boolean;
    open(relpath: string): Promise<void>;
  };

  disposables: DisposableStore;
}

// ─── Plugin entry point ──────────────────────────────────────────────────────

/**
 * Shape a script plugin's default export must satisfy. The host calls
 * the lifecycle hooks in this order:
 *
 *   loadScriptPlugin → onInit → onStart → (dispatch…) → onStop
 *
 * Failures in any hook are logged to the Running Extensions settings
 * tab but do not propagate to other plugins — the contribution bridge
 * keeps going with the next plugin in the snapshot.
 */
export interface ScriptPlugin {
  dispatch(
    handlerId: number,
    args: unknown,
    ctx: NexusPluginContext,
  ): unknown | Promise<unknown>;
  onInit?(ctx: NexusPluginContext): void | Promise<void>;
  onStart?(ctx: NexusPluginContext): void | Promise<void>;
  onStop?(ctx: NexusPluginContext): void | Promise<void>;
}
