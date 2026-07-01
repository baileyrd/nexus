// src/types/plugin.ts
//
// Compat re-export shim — phased move to `@nexus/extension-api` per
// WI-20 / docs/planning/PHASE-1-IMPLEMENTATION-PLAN.md §5.
//
// Existing shell imports continue to work unchanged; new code should
// import directly from `@nexus/extension-api` once the package is
// wired into the build (workspace + path mapping land in a follow-up
// commit). Until then we re-export via a relative path so the shell
// typecheck is self-contained.
//
// Two buckets of types live here:
//
//   1. Portable shapes — re-exported from the extension-api package
//      (manifest, contribution DTOs, configuration, kernel envelope,
//      filesystem entries, etc).
//
//   2. Shell-coupled shapes — kept inline below because their bodies
//      reference `SlotId` from `'../registry/SlotRegistry'` or the
//      `workspace` / `viewRegistry` singletons. These can move once
//      the relevant shell internals land in the kernel contract
//      (slot ids: WI-24 / Phase 7).

import type { ComponentType } from 'react'
import type { workspace, viewRegistry } from '../workspace'

// Re-export portable contribution / config / kernel-envelope shapes
// from the extension-api package. `@nexus/extension-api` resolves via
// the pnpm workspace symlink + tsconfig `paths` alias.
//
// `PluginManifest` is NOT re-exported because its `contributes` field
// references the shell-coupled `PluginContributions` aggregate; it is
// declared inline below.
export type {
  CommandContribution,
  MenuContribution,
  KeybindingContribution,
  StatusBarContribution,
  ContextKeyContribution,
  SettingsTabContribution,
  SlotId,
  ViewContribution,
  ConfigSection,
  ConfigSchema,
  KernelEventEnvelope,
  FileEntry,
  FsEvent,
  PlatformAPI,
  PlatformFsAPI,
  PlatformDialogAPI,
  PlatformWindowAPI,
  PlatformShellAPI,
  PlatformDirEntry,
  PlatformOpenFileOptions,
  PlatformOpenDirectoryOptions,
  PlatformSaveFileOptions,
  UriAPI,
  Snippet,
} from '@nexus/extension-api'

import type {
  CommandContribution,
  MenuContribution,
  KeybindingContribution,
  StatusBarContribution,
  ContextKeyContribution,
  SettingsTabContribution,
  SlotId,
  ViewContribution,
  ConfigSection,
  FileEntry,
  FsEvent,
  PlatformAPI,
  UriAPI,
  Snippet,
} from '@nexus/extension-api'

// ─── Shell-coupled manifest + contributions ─────────────────────────────────
//
// `PluginManifest` + `PluginContributions` remain shell-side because
// they aggregate portable contribution DTOs with the `ViewContribution`
// shape; `ViewContribution` and `SlotId` themselves are portable (OI-04)
// and live in `@nexus/extension-api`.

export interface PluginManifest {
  id: string
  name: string
  version: string
  core: boolean
  activationEvents: string[]
  /**
   * Plugin API version the plugin targets. Omit for legacy plugins —
   * the shell logs a one-shot warn and loads them anyway. Mismatched
   * values are rejected with a `PluginApiVersionError` before activation.
   * Mirrors `PLUGIN_API_VERSION` from `@nexus/extension-api` (WI-33).
   */
  apiVersion?: number
  /**
   * Plugin ids this plugin requires to be active before it activates.
   * Accepts two kinds of id:
   *   - Shell plugin ids (`core.*`, `nexus.*`, `community.*`) — the
   *     ExtensionHost ensures these are activated first; missing or
   *     failed deps fail this plugin's activation.
   *   - Kernel plugin ids (`com.nexus.*`) — documentation of cross-tier
   *     coupling. The kernel loads every core plugin synchronously in
   *     `register_all` before the shell mounts, so these are always
   *     available when the shell activates. The host recognises the
   *     prefix and skips the shell-registry lookup — kernel-side
   *     enforcement is in `crates/nexus-plugins/src/loader.rs::check_dependencies`.
   */
  dependsOn?: string[]
  contributes?: PluginContributions
  /**
   * SH-020: Whether this plugin should run in popout windows.
   * Defaults to true when absent. Set to false for chrome-only plugins
   * (activity bar, sidebar, status bar, settings, etc.) that contribute
   * to slots the popout shell does not render — loading them in a popout
   * is dead work that inflates boot time.
   */
  popoutCompatible?: boolean
}

/**
 * Shell-side tab record. Adds `pluginId` (filled by the registry) to
 * the portable contribution shape so the UI can tag rail entries with
 * their owning plugin for the Obsidian-style grouping.
 */
export interface SettingsTabEntry extends SettingsTabContribution {
  pluginId: string
}

/**
 * OI-18 — Manifest-declared snippet contribution. Matches the runtime
 * `Snippet` shape from `@nexus/extension-api` so plugins can declare
 * snippets statically (for conflict detection before activation) or
 * register them dynamically in `activate()` via `api.editor.registerSnippet`.
 */
export interface SnippetContribution {
  id:           string
  trigger:      string
  body:         string
  description?: string
  /** When set, this snippet only fires for listed file extensions (e.g. `["md", "mdx"]`). */
  fileTypes?:   string[]
}

export interface PluginContributions {
  commands?: CommandContribution[]
  views?: ViewContribution[]
  menus?: MenuContribution[]
  keybindings?: KeybindingContribution[]
  statusBarItems?: StatusBarContribution[]
  configuration?: ConfigSection
  contextKeys?: ContextKeyContribution[]
  settingsTabs?: SettingsTabContribution[]
  /** OI-18 — snippets declared statically in the manifest. */
  snippets?: SnippetContribution[]
}

// ─── Plugin contract ──────────────────────────────────────────────────────────

export interface Plugin {
  manifest: PluginManifest
  activate: (api: PluginAPI) => void | Promise<void>
  deactivate?: () => void | Promise<void>
}

// ─── Plugin API ───────────────────────────────────────────────────────────────
//
// `PluginAPI` references the `workspace` and `viewRegistry` singletons
// from `../workspace`, so it stays shell-side. Individual sub-API
// shapes that ARE portable still live in this file (rather than the
// package) because they're consumed exclusively through the
// shell-coupled `PluginAPI` aggregate; moving them piecemeal would
// require splitting the file without simplifying any consumer.

export interface PluginAPI {
  /**
   * Stable id of the plugin this API instance was built for (#187).
   * Baked in by `buildPluginAPI` from the validated
   * `BuildOptions.pluginId` — the same string that namespaces
   * `storage` keys and tags `PluginRegistry` ownership, so it is
   * host-asserted, never self-asserted. Mirrors
   * `SandboxedPluginContext.pluginId` so both tiers satisfy the common
   * `NexusPluginContext` contract (see
   * `shell/src/types/contractConformance.test-d.ts`).
   */
  readonly pluginId: string
  commands: CommandsAPI
  /**
   * Chrome-slot registration only (titleBar, activityBar, statusBarLeft,
   * statusBarRight, overlay, paneMode). For movable panes, use
   * `viewRegistry` + `workspace` instead — see docs/architecture/leaf-architecture.md.
   */
  views: ViewsAPI
  /**
   * The Leaf/View workspace facade. Plugins register view creators with
   * `viewRegistry.register(type, creator)` and create/reveal leaves with
   * `workspace.ensureLeafOfType(type, side)` + `workspace.revealLeaf(leaf)`.
   */
  workspace: typeof workspace
  viewRegistry: typeof viewRegistry
  context: ContextAPI
  events: EventsAPI
  storage: StorageAPI
  statusBar: StatusBarAPI
  configuration: ConfigurationAPI
  /**
   * Live-rebind keybinding overrides on behalf of the calling plugin
   * (FU-9). Pushes are tagged with the plugin id; on plugin unload,
   * `PluginRegistry.unregisterAll` clears any override the plugin
   * pushed UNLESS the user has since changed it via the Settings UI.
   * Mirrors `KeybindingRegistry.setOverride/clearOverride` (which the
   * Settings UI continues to use directly for user-driven edits).
   */
  keybindings: KeybindingsAPI
  notifications: NotificationsAPI
  fs: FilesystemAPI
  kernel: KernelAPI
  /**
   * OS-level capabilities (filesystem, dialogs, window controls,
   * open-in-default-app). Wraps `@tauri-apps/*` so plugins never import
   * those directly — see WI-23 import-hygiene guardrail.
   */
  platform: PlatformAPI
  activityBar: ActivityBarAPI
  input: InputAPI
  /**
   * Settings modal extension point (OI-01). Plugins register a
   * tab renderer; the shell draws the rail entry and calls the
   * renderer when the user selects that tab. Metadata (title, icon,
   * group, priority) can be declared in the manifest via
   * `settings_tabs`, in which case the rail entry appears even before
   * the plugin activates.
   */
  settings: SettingsAPI
  /**
   * Custom URI scheme registry — `api.uri.register('nexus', handler)`
   * routes deep links of the form `nexus://...` to the handler. Auto-
   * cleaned on plugin deactivate via `PluginRegistry.trackSubscription`.
   * See WI-13 / Phase 2 §5.3.
   */
  uri: UriAPI
  /**
   * Active-editor accessor surface (OI-14). `editor.active()` returns
   * the currently focused tab's `{ relpath, revision }` snapshot, or
   * `null` when no tab is open. `editor.onChange(handler)` subscribes
   * to changes in the active tab or its `sessionRevision`; the
   * returned disposer is auto-swept on plugin unload via
   * `PluginRegistry.trackSubscription`. Lets plugins read the live
   * editor state without `kernel.invoke('com.nexus.editor', ...)`.
   */
  editor: EditorAPI
  /** Only available to core plugins (core: true) */
  internal?: InternalAPI
}

export interface ActiveEditor {
  /** Forge-relative path of the active tab. */
  relpath: string
  /**
   * Opaque version token. Increments on every local or remote edit
   * to the active buffer (sourced from `useEditorStore.sessionRevision`).
   * Plugins should treat it as a cache-invalidation handle, not a byte
   * count.
   */
  revision: number
}

export interface EditorAPI {
  /** Snapshot of the active editor tab, or `null` when none is open. */
  active(): ActiveEditor | null
  /**
   * Subscribe to changes in the active tab. Fires when the user
   * switches tabs, when the active buffer's revision advances, or
   * when there is no longer an active tab. The returned disposer is
   * idempotent and auto-swept on plugin unload.
   */
  onChange(handler: (active: ActiveEditor | null) => void): () => void
  /**
   * BL-008 — register a renderer for a fenced code block language
   * tag. See `@nexus/extension-api`'s `EditorAPI` for the full
   * contract. The shell-side implementation forwards to
   * `fencedCodeRegistry.register` and tracks the disposer through
   * `PluginRegistry.trackSubscription` so plugin deactivate sweeps it.
   */
  registerFencedCodeRenderer(
    language: string,
    renderer: FencedRenderer,
  ): () => void
  /**
   * OI-18 — register a text-expansion snippet. Duplicates of an
   * existing trigger are stored and surfaced as a conflict in
   * Settings → Snippets; last-registered wins for expansion.
   * The returned disposer removes the snippet and re-evaluates
   * conflicts. Auto-swept on plugin unload.
   */
  registerSnippet(snippet: Snippet): () => void
}

export type FencedRenderResult = HTMLElement | Promise<HTMLElement>
export type FencedRenderer = (source: string, info: string) => FencedRenderResult

export interface CommandsAPI {
  register(id: string, handler: (...args: unknown[]) => unknown): void
  execute(id: string, ...args: unknown[]): Promise<unknown>
  all(): CommandEntry[]
}

export interface CommandEntry {
  id: string
  title: string
  category?: string
  keybinding?: string
  pluginId: string
}

export interface ViewsAPI {
  register(viewId: string, config: {
    slot: SlotId
    // Heterogeneous component registry — see PluginAPI.ts for rationale.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    component: ComponentType<any>
    priority?: number
  }): void
}

export interface ContextAPI {
  set(key: string, value: unknown): void
  get(key: string): unknown
  evaluate(expression: string): boolean
}

export interface EventsAPI {
  on<T = unknown>(event: string, handler: (payload: T) => void): () => void
  emit<T = unknown>(event: string, payload: T): void
}

export interface StorageAPI {
  get(key: string): string | null
  set(key: string, value: string): void
  delete(key: string): void
  clear(): void
}

export interface StatusBarItemHandle {
  /** Update the plain-text label (used when `content` is not set). */
  text: string
  /** Update the React content — takes precedence over `text`. */
  content: import('react').ReactNode
  tooltip: string
  dispose(): void
}

export interface StatusBarAPI {
  createItem(config: {
    id: string
    slot: 'left' | 'right'
    priority: number
    /** Plain text. Required unless `content` is provided. */
    text?: string
    /** Rich React node — dots, <code> badges, icons. Wins over `text`. */
    content?: import('react').ReactNode
    tooltip?: string
    command?: string
    /** Extra class names (e.g. `'ember'` for accent-colored sync dot). */
    className?: string
  }): StatusBarItemHandle
}

export interface ConfigurationAPI {
  register(section: ConfigSection): void
  getValue<T>(key: string, defaultValue: T): T
  setValue(key: string, value: unknown): void
  onChange(key: string, handler: (newValue: unknown) => void): () => void
}

export interface KeybindingsAPI {
  /**
   * Apply or replace the active chord for `commandId` at runtime. The
   * override is normalised, persisted via the registry's storage
   * adapter, and tagged internally so plugin deactivation can sweep it.
   */
  setOverride(commandId: string, chord: string): Promise<void>
  /**
   * Drop the override the plugin previously pushed for `commandId`,
   * reverting to the manifest default. No-ops when nothing is set.
   */
  clearOverride(commandId: string): Promise<void>
}

export interface NotificationsAPI {
  show(notification: {
    message: string
    type?: 'info' | 'warning' | 'error' | 'success'
    duration?: number
    actions?: Array<{ label: string; command: string }>
  }): void
}

export interface FilesystemAPI {
  read(path: string): Promise<string>
  write(path: string, content: string): Promise<void>
  list(path: string): Promise<FileEntry[]>
  watch(path: string, handler: (event: FsEvent) => void): Promise<() => void>
  exists(path: string): Promise<boolean>
  mkdir(path: string): Promise<void>
  delete(path: string): Promise<void>
  rename(from: string, to: string): Promise<void>
}

// ─── Kernel bridge ────────────────────────────────────────────────────────────

export interface KernelAPI {
  /**
   * Invoke a kernel plugin handler via `context.ipc_call`. Rejects with a
   * string of the form `"<Variant>: <message>"` mapped from `IpcError`.
   *
   * `timeoutMs` defaults to 30 seconds when omitted.
   */
  invoke<T = unknown>(
    pluginId: string,
    commandId: string,
    args?: unknown,
    timeoutMs?: number,
  ): Promise<T>
  /**
   * Subscribe to kernel custom events whose `type_id` starts with
   * `topicPrefix`. The handler receives the full `type_id` alongside the
   * raw JSON payload so the caller can route across a shared prefix.
   *
   * Returns an unsubscribe function that tears down both the Tauri event
   * listener and the Rust-side forwarder task.
   */
  on<T = unknown>(
    topicPrefix: string,
    handler: (topic: string, payload: T) => void,
  ): Promise<() => void>
  /** True once `boot_kernel` has succeeded and no shutdown has happened since. */
  available(): Promise<boolean>
}

export interface SettingsAPI {
  /**
   * Register a React component that draws the right-pane content
   * when the user selects the tab identified by `id`. Optional
   * `meta` overrides or fills in the manifest-declared metadata
   * — useful when the plugin wants to attach a tab without a
   * manifest entry.
   *
   * Calling `registerTab` for a manifest-declared id attaches the
   * renderer to the existing entry; calling it for a fresh id
   * synthesises an entry on the fly.
   */
  registerTab(
    id: string,
    renderer: import('react').ComponentType<Record<string, never>>,
    meta?: Partial<{
      title: string
      icon: string
      priority: number
      group: 'options' | 'core-plugins' | 'community-plugins'
    }>,
  ): void
}

export interface ActivityBarAPI {
  addItem(config: {
    id: string
    icon: string
    /** Optional SVG path `d` for a stroke-only icon (viewBox 0 0 24 24). When present, wins over `icon`. */
    iconPath?: string
    /**
     * Preferred for new items. Names a glyph from `shell/src/icons/`
     * — supports multi-element shapes (search, graph, sparkle, …)
     * that the legacy `iconPath` (single `<path d>`) can't represent.
     * Wins over `iconPath` and `icon` when set. Untyped here to keep
     * `types/plugin.ts` free of an `icons/` import; the activity-bar
     * store narrows to `IconName` at the point of consumption.
     */
    iconName?: string
    title: string
    viewId: string
    priority: number
    /** Where in the bar to render the item. Defaults to 'top'. */
    placement?: 'top' | 'bottom'
    /**
     * If set, clicking this item executes the named command instead of
     * toggling a sidebar view. Intended for action items (e.g. settings).
     */
    command?: string
  }): void
  removeItem(id: string): void
}

export interface PickItem<T = unknown> {
  label: string
  description?: string
  detail?: string
  value: T
}

export interface PickOptions {
  placeholder?: string
  title?: string
}

export interface InputAPI {
  prompt(message: string, placeholder?: string): Promise<string | null>
  confirm(message: string): Promise<boolean>
  /**
   * Show a list-picker modal and resolve with the picked item's
   * `value` (or `null` on cancel / dismiss). Empty `items` resolves
   * immediately with `null` so callers don't need to guard. Backed
   * by the `nexus.pick` plugin's overlay modal.
   */
  pick<T = unknown>(items: PickItem<T>[], options?: PickOptions): Promise<T | null>
}

export interface InternalAPI {
  registerInternalService(name: string, service: unknown): void
  getInternalService<T>(name: string): T
  defineSlot(slotId: string): void
  registry: unknown // PluginRegistry — typed loosely here to avoid circular dep
}
