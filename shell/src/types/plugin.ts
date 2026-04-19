// src/types/plugin.ts
// Core type definitions for the plugin system

import type { ComponentType } from 'react'
import type { SlotId } from '../registry/SlotRegistry'

// ─── Manifest ────────────────────────────────────────────────────────────────

export interface PluginManifest {
  id: string
  name: string
  version: string
  core: boolean
  activationEvents: string[]
  dependsOn?: string[]
  contributes?: PluginContributions
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

export interface CommandContribution {
  id: string
  title: string
  category?: string
  icon?: string
}

export interface ViewContribution {
  id: string
  slot: SlotId
  title: string
  priority?: number
}

export interface MenuContribution {
  menu: string
  command: string
  group?: string
  order?: number
  when?: string
}

export interface KeybindingContribution {
  command: string
  key: string
  mac?: string
  when?: string
}

export interface StatusBarContribution {
  id: string
  slot: 'left' | 'right'
  priority: number
  text: string
}

export interface ContextKeyContribution {
  key: string
  description: string
  type: 'boolean' | 'string' | 'number'
}

// ─── Configuration ────────────────────────────────────────────────────────────

export interface ConfigSection {
  pluginId: string
  title: string
  order: number
  schema: ConfigSchema[]
}

export interface ConfigSchema {
  key: string
  title: string
  description: string
  type: 'boolean' | 'string' | 'number' | 'select' | 'keybinding'
  default: unknown
  options?: string[]
  when?: string
}

// ─── Plugin contract ──────────────────────────────────────────────────────────

export interface Plugin {
  manifest: PluginManifest
  activate: (api: PluginAPI) => void | Promise<void>
  deactivate?: () => void | Promise<void>
}

// ─── Plugin API ───────────────────────────────────────────────────────────────

export interface PluginAPI {
  commands: CommandsAPI
  views: ViewsAPI
  context: ContextAPI
  events: EventsAPI
  storage: StorageAPI
  statusBar: StatusBarAPI
  configuration: ConfigurationAPI
  notifications: NotificationsAPI
  fs: FilesystemAPI
  kernel: KernelAPI
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

export interface FileEntry {
  name: string
  path: string
  isDirectory: boolean
}

export interface FsEvent {
  kind: 'created' | 'modified' | 'deleted' | 'renamed'
  path: string
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

export interface KernelEventEnvelope {
  subscriptionId: string
  topic: string
  payload: unknown
}

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
    title: string
    viewId: string
    priority: number
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
