// src/host/PluginAPI.ts
// Constructs the PluginAPI object handed to each plugin's activate() function.
// Core plugins get api.internal; community plugins do not.

import type {
  PluginAPI,
  ActiveEditor,
  ConfigSection,
  KernelEventEnvelope,
  FencedRenderer,
} from '../types/plugin'
import { fencedCodeRegistry } from '../plugins/nexus/editor/cm/fencedCodeRegistry'
import type { PluginRegistry } from './PluginRegistry'
import { useSlotStore, type SlotId } from '../registry/SlotRegistry'
import { uriHandlerRegistry } from '../registry/UriHandlerRegistry'
import { contextKeyService } from './ContextKeyService'
import { eventBus } from './EventBus'
import { workspace, viewRegistry } from '../workspace'
import { useEditorStore } from '../plugins/nexus/editor/editorStore'
import { computeActiveEditor, activeEditorEquals } from './activeEditor'
import { KernelIpcError } from './KernelIpcError'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  readTextFile,
  writeTextFile,
  readDir,
  exists as fsExists,
  mkdir,
  remove,
  rename,
} from '@tauri-apps/plugin-fs'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { open as openInShell } from '@tauri-apps/plugin-shell'
import type { ComponentType } from 'react'

interface BuildOptions {
  isCore: boolean
  pluginId: string
}

/**
 * Validate a `pluginId` before it is baked into a `PluginAPI` instance.
 *
 * The id flows into:
 *   - `localStorage` keys as `plugin:${pluginId}:${userKey}` — colon-
 *     bearing ids would let a plugin escape its namespace and read
 *     another plugin's storage (e.g. `pluginId="foo:bar"` shares the
 *     `plugin:foo:` prefix with a `foo` plugin).
 *   - `eventBus.emit` payloads (`activityBar:itemAdded`,
 *     `settings:tabsChanged`, …) where the field is read as the
 *     authoritative source.
 *   - `PluginRegistry.track(pluginId, …)` and `trackSubscription` so
 *     `unregisterAll(pluginId)` correctly scopes per-plugin cleanup.
 *
 * Rejecting empty / non-string / colon-bearing ids closes the F-8.1.2
 * "self-asserted pluginId" hole at the host's only ingress point. Real
 * plugin ids in this codebase are dotted (e.g. `com.nexus.editor`,
 * `community.hello-world`) and never contain colons, so this is a
 * non-breaking constraint for shipped plugins.
 */
export function assertValidPluginId(pluginId: unknown): asserts pluginId is string {
  if (typeof pluginId !== 'string') {
    throw new TypeError(
      `[PluginAPI] pluginId must be a string, got ${typeof pluginId}`,
    )
  }
  if (pluginId.length === 0) {
    throw new Error('[PluginAPI] pluginId must not be empty')
  }
  if (pluginId.includes(':')) {
    // `:` is the namespace separator inside `plugin:<id>:<key>`
    // localStorage keys; permitting it would let `pluginId="a:b"` collide
    // with the `a` plugin's namespace.
    throw new Error(
      `[PluginAPI] pluginId must not contain ':' (got '${pluginId}')`,
    )
  }
}

export function buildPluginAPI(
  registry: PluginRegistry,
  opts: BuildOptions
): PluginAPI {
  const { pluginId, isCore } = opts
  assertValidPluginId(pluginId)

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

    // ─── Settings tabs (OI-01) ─────────────────────────────────────────────
    // Plugins register a renderer for a tab id; the shell's settings
    // modal draws the rail entry using metadata from the manifest (or
    // from `meta` when no manifest entry exists) and invokes the
    // renderer when the user selects the tab.
    settings: {
      registerTab(id, renderer, meta) {
        registry.settingsTabs.register(pluginId, id, renderer, meta)
        registry.track(pluginId, `settingsTab:${id}`)
        // Notify the settings panel so plugin-contributed tabs that
        // arrive AFTER `SettingsPanelView` mounted still appear in the
        // rail. Without this, the panel only re-reads the registry on
        // `plugin:activated` — but a plugin can activate, register the
        // tab, and emit `plugin:activated` all in the same tick before
        // the panel's effect has subscribed (race window).
        eventBus.emit('settings:tabsChanged', { pluginId, tabId: id })
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

    // ─── Keybindings (FU-9) ────────────────────────────────────────────────
    // Live-rebind facade. Every push is tagged with `pluginId` inside
    // `PluginRegistry` so plugin deactivation can sweep this plugin's
    // overrides without disturbing user-driven overrides set via the
    // Settings UI.
    keybindings: {
      setOverride(commandId, chord) {
        return registry.setKeybindingOverride(pluginId, commandId, chord)
      },
      clearOverride(commandId) {
        return registry.clearKeybindingOverride(pluginId, commandId)
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
        try {
          return await invoke<T>('kernel_invoke', {
            pluginId,
            commandId,
            args,
            timeoutMs: timeoutMs ?? null,
          })
        } catch (raw) {
          // Post-WI-06 the bridge always returns an `IpcErrorEnvelope` on
          // failure; wrap it so plugins can branch on `err.kind` instead
          // of string-sniffing `err.message`. Non-envelope errors (older
          // bridge builds, JS-side serialization failures, etc.) bubble
          // through unchanged so we don't accidentally swallow shape
          // mismatches.
          if (KernelIpcError.isEnvelope(raw)) {
            throw new KernelIpcError(raw)
          }
          throw raw
        }
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
        // Idempotency guard: the same disposer is invoked both by plugin
        // code (if it stored the returned unsubscribe) AND by
        // `PluginRegistry.unregisterAll` on plugin unload. Without this
        // flag the second call would re-issue `kernel_unsubscribe` (the
        // Rust side tolerates it but logs a warning) — cheap to avoid.
        let disposed = false
        const unsub = () => {
          if (disposed) return
          disposed = true
          // Fire-and-forget: Tauri listener is dropped synchronously; the
          // Rust-side `kernel_unsubscribe` is best-effort (idempotent + logs
          // on failure) and doesn't need to block the caller's teardown.
          unlisten()
          invoke('kernel_unsubscribe', { subscriptionId }).catch((e) =>
            console.warn('[api.kernel.on] unsubscribe failed', e),
          )
        }
        // Track for automatic sweep on plugin unload — without this the
        // Tauri listener would keep firing into a dead handler and the
        // Rust forwarder task would live on. Plugins that explicitly
        // dispose via the returned function still work because `unsub`
        // is idempotent.
        registry.trackSubscription(pluginId, unsub)
        return unsub
      },
      async available(): Promise<boolean> {
        return invoke<boolean>('kernel_is_booted')
      },
    },

    // ─── Platform adapter surface ─────────────────────────────────────────
    // Wraps `@tauri-apps/*` so plugins never import those directly. The
    // WI-23 import-hygiene test only checks `shell/src/plugins/**`; this
    // file lives under `shell/src/host/` and is a permitted Tauri caller.
    platform: {
      fs: {
        readText(path) {
          return readTextFile(path)
        },
        writeText(path, content) {
          return writeTextFile(path, content)
        },
        async readDir(path) {
          const entries = await readDir(path)
          return entries.map((e) => ({
            name: e.name ?? '',
            isDirectory: e.isDirectory ?? false,
          }))
        },
        exists(path) {
          return fsExists(path)
        },
        mkdir(path, options) {
          return mkdir(path, { recursive: options?.recursive ?? true })
        },
        remove(path) {
          return remove(path)
        },
        rename(from, to) {
          return rename(from, to)
        },
      },
      dialog: {
        async openFile(options?: { multiple?: boolean; title?: string; defaultPath?: string; filters?: ReadonlyArray<{ name: string; extensions: ReadonlyArray<string> }> }) {
          // Cast for tauri-plugin-dialog: it expects mutable arrays.
          const result = await openDialog({
            ...(options ?? {}),
            multiple: options?.multiple ?? false,
            directory: false,
            filters: options?.filters?.map((f) => ({ name: f.name, extensions: [...f.extensions] })),
          })
          return result as string | string[] | null
        },
        async openDirectory(options?: { multiple?: boolean; title?: string; defaultPath?: string }) {
          const result = await openDialog({
            ...(options ?? {}),
            multiple: options?.multiple ?? false,
            directory: true,
          })
          return result as string | string[] | null
        },
        async saveFile(options) {
          const result = await saveDialog({
            ...(options ?? {}),
            filters: options?.filters?.map((f) => ({ name: f.name, extensions: [...f.extensions] })),
          })
          return result
        },
      } as PluginAPI['platform']['dialog'],
      window: {
        async minimize() {
          await getCurrentWindow().minimize()
        },
        async toggleMaximize() {
          await getCurrentWindow().toggleMaximize()
        },
        async close() {
          await getCurrentWindow().close()
        },
        async isMaximized() {
          return getCurrentWindow().isMaximized()
        },
        async onResize(handler) {
          // `onResized` is the modern Tauri v2 listener; it returns an
          // unlisten promise. We track the resulting disposer through the
          // plugin's subscription registry so deactivation sweeps it (mirrors
          // `kernel.on`'s handling).
          const unlisten = await getCurrentWindow().onResized(() => handler())
          let disposed = false
          const unsub = () => {
            if (disposed) return
            disposed = true
            unlisten()
          }
          registry.trackSubscription(pluginId, unsub)
          return unsub
        },
      },
      shell: {
        async openExternal(target) {
          await openInShell(target)
        },
      },
    },

    // ─── Activity bar ──────────────────────────────────────────────────────
    // Items are tracked by plugin id so `PluginRegistry.unregisterAll`
    // can sweep them on plugin unload — without this, disabling a plugin
    // (e.g. via Settings → Plugins) leaves its rail icons visible.
    activityBar: {
      addItem(config) {
        eventBus.emit('activityBar:itemAdded', { ...config, pluginId })
        registry.track(pluginId, `activityBar:${config.id}`)
      },
      removeItem(id) {
        eventBus.emit('activityBar:itemRemoved', { id })
      },
    },

    // ─── URI handlers (WI-13) ──────────────────────────────────────────────
    // Deep-link dispatch surface. `register(scheme, handler)` claims a
    // scheme; a Tauri-side bridge (deferred — see WI-13 report) calls
    // `uriHandlerRegistry.dispatch(url)` with each incoming URL. The
    // returned unsub is tracked so plugin deactivation sweeps the
    // registration automatically (mirrors `kernel.on`).
    uri: {
      register(scheme, handler) {
        const unsub = uriHandlerRegistry.register(scheme, pluginId, handler)
        registry.trackSubscription(pluginId, unsub)
        return unsub
      },
    },

    // ─── Active editor (OI-14) ─────────────────────────────────────────────
    // Typed read-only surface over `useEditorStore` so plugins don't reach
    // into the editor's internal command ids (`com.nexus.editor::open` etc.)
    // for the most basic question — "what's the user looking at?". The
    // `revision` field is sourced from the kernel's `sessionRevision` and
    // is opaque (a cache key, not a byte count). `onChange` fires when the
    // active tab changes OR the active buffer's revision advances; the
    // returned disposer is idempotent and tracked so plugin unload sweeps
    // it (mirrors `kernel.on`'s subscription handling).
    editor: {
      active(): ActiveEditor | null {
        return computeActiveEditor(useEditorStore.getState())
      },
      onChange(handler: (active: ActiveEditor | null) => void): () => void {
        let lastSnapshot = computeActiveEditor(useEditorStore.getState())
        const unsubInner = useEditorStore.subscribe((state) => {
          const next = computeActiveEditor(state)
          if (activeEditorEquals(next, lastSnapshot)) return
          lastSnapshot = next
          try {
            handler(next)
          } catch (err) {
            console.warn(`[api.editor.onChange] handler for ${pluginId} threw`, err)
          }
        })
        let disposed = false
        const unsub = () => {
          if (disposed) return
          disposed = true
          unsubInner()
        }
        registry.trackSubscription(pluginId, unsub)
        return unsub
      },
      registerFencedCodeRenderer(language: string, renderer: FencedRenderer): () => void {
        const inner = fencedCodeRegistry.register(language, renderer)
        let disposed = false
        const unsub = () => {
          if (disposed) return
          disposed = true
          inner()
        }
        registry.trackSubscription(pluginId, unsub)
        return unsub
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
