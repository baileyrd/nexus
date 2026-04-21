// src/host/PluginAPI.ts
// Constructs the PluginAPI object handed to each plugin's activate() function.
// Core plugins get api.internal; community plugins do not.

import type { PluginAPI, ConfigSection, KernelEventEnvelope } from '../types/plugin'
import type { PluginRegistry } from './PluginRegistry'
import { useSlotStore, type SlotId } from '../registry/SlotRegistry'
import { contextKeyService } from './ContextKeyService'
import { eventBus } from './EventBus'
import { workspace, viewRegistry } from '../workspace'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { ComponentType } from 'react'

interface BuildOptions {
  isCore: boolean
  pluginId: string
}

export function buildPluginAPI(
  registry: PluginRegistry,
  opts: BuildOptions
): PluginAPI {
  const { pluginId, isCore } = opts

  const api: PluginAPI = {
    // ─── Commands ──────────────────────────────────────────────────────────
    commands: {
      register(id, handler) {
        registry.commands.register(pluginId, id, handler)
        registry.track(pluginId, `command:${id}`)
      },
      execute(id, ...args) {
        return registry.commands.execute(id, ...args)
      },
      all() {
        // Back-fill `keybinding` from the KeybindingRegistry so callers
        // like the command palette can render a keybinding pill without
        // needing two API calls. CommandRegistry + KeybindingRegistry
        // live separately; the join happens here so neither has to
        // know about the other.
        const bindings = registry.keybindings.all()
        const byCommand = new Map<string, string>()
        for (const b of bindings) {
          if (!byCommand.has(b.commandId)) byCommand.set(b.commandId, b.chord)
        }
        return registry.commands.all().map((cmd) => ({
          ...cmd,
          keybinding: byCommand.get(cmd.id) ?? cmd.keybinding,
        }))
      },
    },

    // ─── Views ─────────────────────────────────────────────────────────────
    views: {
      register(viewId, config: { slot: SlotId; component: ComponentType<any>; priority?: number }) {
        useSlotStore.getState().register(config.slot, {
          id: viewId,
          pluginId,
          component: config.component,
          priority: config.priority ?? 50,
        })
        registry.track(pluginId, `slot:${viewId}`)
      },
    },

    // ─── Workspace / ViewRegistry ─────────────────────────────────────────
    // Injected so plugins can reach the Leaf-based workspace facade via their
    // `api` argument without importing shell internals directly. See
    // docs/leaf-architecture.md.
    workspace,
    viewRegistry,

    // ─── Context keys ───────────────────────────────────────────────────────
    context: {
      set(key, value) {
        contextKeyService.set(key, value)
      },
      get(key) {
        return contextKeyService.get(key)
      },
      evaluate(expression) {
        return contextKeyService.evaluate(expression)
      },
    },

    // ─── Events ────────────────────────────────────────────────────────────
    events: {
      on(event, handler) {
        return eventBus.on(event, handler)
      },
      emit(event, payload) {
        eventBus.emit(event, payload)
      },
    },

    // ─── Storage ───────────────────────────────────────────────────────────
    storage: {
      get(key) {
        return localStorage.getItem(`plugin:${pluginId}:${key}`)
      },
      set(key, value) {
        localStorage.setItem(`plugin:${pluginId}:${key}`, value)
      },
      delete(key) {
        localStorage.removeItem(`plugin:${pluginId}:${key}`)
      },
      clear() {
        const prefix = `plugin:${pluginId}:`
        Object.keys(localStorage)
          .filter(k => k.startsWith(prefix))
          .forEach(k => localStorage.removeItem(k))
      },
    },

    // ─── Status bar ────────────────────────────────────────────────────────
    statusBar: {
      createItem(config) {
        const handle = registry.statusBar.create(pluginId, config)
        registry.track(pluginId, `statusBar:${config.id}`)
        return handle
      },
    },

    // ─── Configuration ─────────────────────────────────────────────────────
    // Available after core.configuration-service has loaded
    configuration: {
      register(section: ConfigSection) {
        registry.config.register(section)
        registry.track(pluginId, `config:${section.pluginId}`)
      },
      getValue<T>(key: string, defaultValue: T): T {
        try {
          const store = registry.getService<{ get: (k: string, d: T) => T }>('configStore')
          return store.get(key, defaultValue)
        } catch {
          return defaultValue
        }
      },
      setValue(key: string, value: unknown) {
        try {
          const store = registry.getService<{ set: (k: string, v: unknown) => void }>('configStore')
          store.set(key, value)
        } catch {
          console.warn('[PluginAPI] configuration-service not loaded yet')
        }
      },
      onChange(key: string, handler: (v: unknown) => void) {
        // Subscribes to config store changes for a specific key
        // Implementation depends on configStore's subscription model
        return eventBus.on(`config:changed:${key}`, handler)
      },
    },

    // ─── Notifications ─────────────────────────────────────────────────────
    // Available after core.notification-service has loaded
    notifications: {
      show(notification) {
        try {
          const queue = registry.getService<{
            push: (n: typeof notification) => void
          }>('notificationQueue')
          queue.push(notification)
        } catch {
          // Fallback to console if notification service isn't loaded
          console.info(`[Notification] ${notification.message}`)
        }
      },
    },

    // ─── Filesystem ────────────────────────────────────────────────────────
    // Available after core.filesystem-service has loaded
    fs: {
      async read(path) {
        const svc = registry.getService<{ read: (p: string) => Promise<string> }>('fsService')
        return svc.read(path)
      },
      async write(path, content) {
        const svc = registry.getService<{ write: (p: string, c: string) => Promise<void> }>('fsService')
        return svc.write(path, content)
      },
      async list(path) {
        const svc = registry.getService<{ list: (p: string) => Promise<unknown[]> }>('fsService')
        return svc.list(path) as ReturnType<PluginAPI['fs']['list']>
      },
      async watch(path, handler) {
        const svc = registry.getService<{ watch: (p: string, h: typeof handler) => Promise<() => void> }>('fsService')
        return svc.watch(path, handler)
      },
      async exists(path) {
        const svc = registry.getService<{ exists: (p: string) => Promise<boolean> }>('fsService')
        return svc.exists(path)
      },
      async mkdir(path) {
        const svc = registry.getService<{ mkdir: (p: string) => Promise<void> }>('fsService')
        return svc.mkdir(path)
      },
      async delete(path) {
        const svc = registry.getService<{ delete: (p: string) => Promise<void> }>('fsService')
        return svc.delete(path)
      },
      async rename(from, to) {
        const svc = registry.getService<{ rename: (f: string, t: string) => Promise<void> }>('fsService')
        return svc.rename(from, to)
      },
    },

    // ─── Kernel bridge ─────────────────────────────────────────────────────
    // Wraps the Tauri commands registered in `src-tauri/src/bridge.rs`. Every
    // call errors with `"kernel not booted"` until `boot_kernel` succeeds on
    // workspace pick.
    kernel: {
      async invoke<T = unknown>(
        pluginId: string,
        commandId: string,
        args: unknown = {},
        timeoutMs?: number,
      ): Promise<T> {
        return invoke<T>('kernel_invoke', {
          pluginId,
          commandId,
          args,
          timeoutMs: timeoutMs ?? null,
        })
      },
      async on<T = unknown>(
        topicPrefix: string,
        handler: (topic: string, payload: T) => void,
      ): Promise<() => void> {
        const subscriptionId = await invoke<string>('kernel_subscribe', { topicPrefix })
        const unlisten = await listen<KernelEventEnvelope>('kernel:event', (ev) => {
          if (ev.payload.subscriptionId === subscriptionId) {
            handler(ev.payload.topic, ev.payload.payload as T)
          }
        })
        return () => {
          // Fire-and-forget: Tauri listener is dropped synchronously; the
          // Rust-side `kernel_unsubscribe` is best-effort (idempotent + logs
          // on failure) and doesn't need to block the caller's teardown.
          unlisten()
          invoke('kernel_unsubscribe', { subscriptionId }).catch((e) =>
            console.warn('[api.kernel.on] unsubscribe failed', e),
          )
        }
      },
      async available(): Promise<boolean> {
        return invoke<boolean>('kernel_is_booted')
      },
    },

    // ─── Activity bar ──────────────────────────────────────────────────────
    activityBar: {
      addItem(config) {
        eventBus.emit('activityBar:itemAdded', { ...config, pluginId })
      },
      removeItem(id) {
        eventBus.emit('activityBar:itemRemoved', { id })
      },
    },

    // ─── Input ─────────────────────────────────────────────────────────────
    input: {
      async prompt(message, placeholder) {
        // Simple browser prompt fallback — replace with custom modal UI
        const result = window.prompt(message, placeholder ?? '')
        return result
      },
      async confirm(message) {
        // Routes into nexus.confirm's overlay modal so users get a
        // styled dialog instead of the platform popup. Lazy import
        // breaks the circular host → plugin dep that a top-level
        // import would create (PluginAPI is built before plugins
        // load, but `requestConfirm` only touches a Zustand store
        // that has no init-time work).
        const { requestConfirm } = await import('../plugins/nexus/confirm/confirmStore')
        return requestConfirm(message)
      },
    },
  }

  // ─── Internal API — core plugins only ─────────────────────────────────────
  if (isCore) {
    api.internal = {
      registerInternalService(name, service) {
        registry.registerService(name, service)
      },
      getInternalService<T>(name: string): T {
        return registry.getService<T>(name)
      },
      defineSlot(_slotId: string) {
        // Slot IDs are currently a union type — extending at runtime
        // would require dynamic SlotId handling. Documented as future work.
        console.warn('[PluginAPI] defineSlot is not yet implemented')
      },
      registry,
    }
  }

  return api
}
