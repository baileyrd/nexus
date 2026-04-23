// src/types/plugin.ts
//
// Compat re-export shim — phased move to `@nexus/extension-api` per
// WI-20 / docs/PHASE-1-IMPLEMENTATION-PLAN.md §5.
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
import type { SlotId } from '../registry/SlotRegistry'
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
} from '@nexus/extension-api'

import type {
  CommandContribution,
  MenuContribution,
  KeybindingContribution,
  StatusBarContribution,
  ContextKeyContribution,
  ConfigSection,
  FileEntry,
  FsEvent,
  PlatformAPI,
} from '@nexus/extension-api'

// ─── Shell-coupled manifest + contributions ─────────────────────────────────
//
// `ViewContribution` references `SlotId`, which is a shell-internal
// type from `registry/SlotRegistry`. TODO: promote to kernel contract
// (WI-24 / Phase 7) and move into the extension-api package; at that
// point `PluginContributions` and `PluginManifest` follow it.

export interface PluginManifest {
  id: string
  name: string
  version: string
  core: boolean
  activationEvents: string[]
  dependsOn?: string[]
  contributes?: PluginContributions
}

export interface ViewContribution {
  id: string
  slot: SlotId
  title: string
  priority?: number
}

export interface PluginContributions {
  commands?: CommandContribution[]
  views?: ViewContribution[]
  menus?: MenuContribution[]
  keybindings?: KeybindingContribution[]
  statusBarItems?: StatusBarContribution[]
  configuration?: ConfigSection
  contextKeys?: ContextKeyContribution[]
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
  commands: CommandsAPI
  /**
   * Chrome-slot registration only (titleBar, activityBar, statusBarLeft,
   * statusBarRight, overlay, paneMode). For movable panes, use
   * `viewRegistry` + `workspace` instead — see docs/leaf-architecture.md.
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
  /** Only available to core plugins (core: true) */
  internal?: InternalAPI
}

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

export interface InputAPI {
  prompt(message: string, placeholder?: string): Promise<string | null>
  confirm(message: string): Promise<boolean>
}

export interface InternalAPI {
  registerInternalService(name: string, service: unknown): void
  getInternalService<T>(name: string): T
  defineSlot(slotId: string): void
  registry: unknown // PluginRegistry — typed loosely here to avoid circular dep
}
