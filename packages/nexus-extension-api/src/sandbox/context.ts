/**
 * Plugin-side API surface for sandboxed community plugins (WI-30c).
 *
 * This is the object a plugin's `activate(ctx)` receives when loaded
 * inside a null-origin iframe sandbox. It deliberately mirrors the
 * first-party `PluginAPI` (shell/src/host/PluginAPI.ts) but only the
 * subset that can cross a `postMessage` boundary — see
 * `docs/wi30-sandbox-design.md` §5.2 for the full method catalog and
 * §6 for the UI-contribution constraint.
 *
 * ## Differences from first-party `PluginAPI`
 *
 * 1. **Several synchronous methods become async.** Anything that goes
 *    through the RPC channel pays a `postMessage` round-trip, so
 *    `storage.get/set/delete`, `notifications.show`, `context.*`,
 *    `statusBar.createItem`, `events.on` (host-bridged events only),
 *    etc. return `Promise<T>`. The per-method JSDoc flags the async
 *    change; plugin authors must `await` where callers of `PluginAPI`
 *    today do not.
 *
 * 2. **React refs don't cross.** `views.registerPanel` takes a
 *    `render()` that returns a {@link PanelNode} tree only — no
 *    `ComponentType`. The host re-invokes `render()` across RPC when
 *    it needs a refresh.
 *
 * 3. **Certain APIs are intentionally absent** (see "Not exposed"
 *    section below). Reach them through supported RPC methods or
 *    wait for a later iteration.
 *
 * ## Not exposed
 *
 * - `workspace` / `viewRegistry` singletons — live object references
 *   that can't cross `postMessage`. Sandboxed plugins that need to
 *   contribute UI should use {@link SandboxedPluginContext.views.registerPanel}
 *   for declarative panels, or route mutations through
 *   {@link SandboxedPluginContext.commands.execute} against first-party
 *   command ids (which already enforce capabilities).
 * - `fs` (the service-plugin-backed `FilesystemAPI`) — redundant with
 *   {@link PlatformAPI} (`ctx.platform.fs.*`). Pick one; the platform
 *   adapter is the sandbox-supported chokepoint.
 * - `configuration` — TODO for a later iteration. Sandboxed plugins
 *   persist their own state via {@link SandboxedPluginContext.storage}
 *   until the configuration bridge lands.
 * - `internal` — core/first-party only; sandboxed plugins never see it.
 * - `editor.registerBlockType` / decorations / keybindings / snippets /
 *   MDX — require first-party hooks into the editor realm. Out of
 *   scope for Phase 3c.
 */

import type { Disposable, PanelNode, PlatformAPI } from '../index';

// ─── Contribution config types (sandbox-safe subset) ─────────────────────────
//
// These mirror the shapes in shell/src/types/plugin.ts but scrub every
// field that carries a React node — sandboxed plugins can't transfer
// closures across postMessage, so `content`/`actions` with callbacks
// drop out. Kept local to sandbox/ so the top-level barrel stays
// React-free.

/** Activity-bar item config — sandbox-safe subset of `ActivityBarAPI.addItem`. */
export interface ActivityBarItemConfig {
  id: string;
  /** Inline icon identifier the host resolves. */
  icon: string;
  /** Optional SVG path `d` for a stroke-only icon (viewBox 0 0 24 24). */
  iconPath?: string;
  /** Preferred glyph name from `shell/src/icons/`. Wins over `iconPath`/`icon`. */
  iconName?: string;
  title: string;
  /** The view id to toggle when the item is clicked — must be a panel registered via {@link SandboxedPluginContext.views.registerPanel}. */
  viewId: string;
  priority: number;
  placement?: 'top' | 'bottom';
  /** Execute a command instead of toggling a view (e.g. settings action). */
  command?: string;
}

/** Status-bar item config — sandbox-safe subset (no React `content`). */
export interface StatusBarItemConfig {
  id: string;
  slot: 'left' | 'right';
  priority: number;
  text: string;
  tooltip?: string;
  /** Command id to execute on click. */
  command?: string;
  /** Extra class names for accent colors / badges. */
  className?: string;
}

/**
 * Live handle returned from {@link SandboxedPluginContext.statusBar.createItem}.
 *
 * Property writes (`handle.text = 'foo'`) are not available over RPC —
 * the guest-side proxy fires and forgets async updates. Use the
 * {@link StatusBarItemHandle.update} method instead.
 */
export interface StatusBarItemHandle {
  readonly id: string;
  /** Update mutable fields. Async — crosses postMessage. */
  update(patch: Partial<Pick<StatusBarItemConfig, 'text' | 'tooltip' | 'command' | 'className'>>): Promise<void>;
  /** Remove the item from the status bar. Idempotent. */
  dispose(): Promise<void>;
}

// ─── The context itself ──────────────────────────────────────────────────────

/**
 * Object handed to a sandboxed plugin's `activate(ctx)` hook.
 *
 * Instances are built inside the iframe by `bootstrapSandboxedPlugin`
 * and marshal every call as an outbound RPC envelope; plugin authors
 * should treat this as an opaque contract and never `new` it.
 */
export interface SandboxedPluginContext {
  /** Stable plugin identifier from the manifest. */
  readonly pluginId: string;

  // ─── Commands ──────────────────────────────────────────────────────────
  commands: {
    /**
     * Register a command handler. The host receives a `handlerId`
     * token instead of the closure; when another plugin (or the host)
     * invokes the command, the host posts `{handlerId, args}` back
     * into this iframe, which dispatches to the stored closure.
     *
     * Disposing is synchronous from the plugin's point of view — the
     * unregister RPC is fire-and-forget.
     */
    register(id: string, handler: (...args: unknown[]) => unknown): Disposable;
    /**
     * Execute a command by id. **Async** (crosses postMessage).
     */
    execute(id: string, ...args: unknown[]): Promise<unknown>;
  };

  // ─── Kernel bridge ─────────────────────────────────────────────────────
  kernel: {
    /**
     * Invoke a kernel-plugin command. **Already async in PluginAPI** —
     * semantics unchanged. The promise rejects with an
     * {@link import('../generated/IpcErrorEnvelope').IpcErrorEnvelope}
     * string on timeout, capability denial, or plugin crash.
     */
    invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
      timeoutMs?: number,
    ): Promise<T>;
    /**
     * Subscribe to kernel custom events. **Async** (was async in
     * `PluginAPI` too — subscription id round-trips to the host).
     * The returned {@link Disposable} is idempotent on both sides.
     */
    on<T = unknown>(
      topicPrefix: string,
      handler: (topic: string, payload: T) => void,
    ): Promise<Disposable>;
  };

  // ─── Platform adapters ─────────────────────────────────────────────────
  /**
   * OS-level capabilities — filesystem, dialogs, window controls,
   * open-in-default-app. Identical shape to the first-party
   * {@link PlatformAPI}; every method was already async so no
   * sync-to-async conversion is needed.
   *
   * Every call is capability-gated host-side (see §5.3 of the design).
   */
  platform: PlatformAPI;

  // ─── Plugin-to-plugin events ───────────────────────────────────────────
  events: {
    /**
     * Subscribe to an in-app event. Synchronous in
     * {@link import('../index').NexusPluginContext}'s first-party
     * counterpart; **async here** because the subscription id comes
     * back from the host. The returned `Disposable` is idempotent.
     *
     * NOTE: the returned Disposable is a sync callback; the _initial_
     * subscribe is async and happens behind a promise internally. If
     * your plugin needs to know the subscription is live before
     * emitting, await {@link events.emit} (which is also async).
     */
    on<T = unknown>(event: string, handler: (payload: T) => void): Disposable;
    /** Emit an event. **Async** (crosses postMessage). */
    emit<T = unknown>(event: string, payload: T): void;
  };

  // ─── Per-plugin key/value storage ──────────────────────────────────────
  /**
   * Namespaced key/value store. All three methods are **async** in the
   * sandbox (were synchronous in `PluginAPI`'s `StorageAPI`) because
   * reads/writes cross `postMessage`. Values are JSON-serialisable.
   */
  storage: {
    get(key: string): Promise<string | null>;
    set(key: string, value: string): Promise<void>;
    delete(key: string): Promise<void>;
  };

  // ─── Notifications ─────────────────────────────────────────────────────
  /**
   * Fire a toast. **Async** (was synchronous in `NotificationsAPI`).
   *
   * `actions` are omitted from the sandbox shape because each entry
   * carried a command-id that the host dispatches — keeping the
   * minimal shape for now; add back when a plugin demands it.
   */
  notifications: {
    show(notification: {
      message: string;
      type?: 'info' | 'warning' | 'error' | 'success';
      duration?: number;
    }): Promise<void>;
  };

  // ─── Context keys (when-expression store) ──────────────────────────────
  /**
   * Per-plugin context keys for menu/keybinding `when` clauses. All
   * three methods are **async** (were synchronous in the first-party
   * `ContextAPI`) — every read/write is an RPC.
   */
  context: {
    set(key: string, value: unknown): Promise<void>;
    get(key: string): Promise<unknown>;
    evaluate(expression: string): Promise<boolean>;
  };

  // ─── UI views ──────────────────────────────────────────────────────────
  views: {
    /**
     * Register a declarative panel (PanelNode tree only — sandboxed
     * plugins cannot return React components across `postMessage`).
     *
     * The host re-invokes `render()` via RPC whenever it needs a
     * fresh tree. Dispose the returned handle to remove the panel.
     *
     * See `docs/wi30-sandbox-design.md` §6 for why React components
     * are off-limits and why `PanelNode` is the chosen contract.
     */
    registerPanel(viewId: string, render: () => PanelNode): Disposable;
  };

  // ─── Input ─────────────────────────────────────────────────────────────
  /**
   * Prompts delegate to host-rendered modals. Both methods were already
   * async in `InputAPI`; unchanged here.
   */
  input: {
    prompt(message: string, placeholder?: string): Promise<string | null>;
    confirm(message: string): Promise<boolean>;
  };

  // ─── URI handlers ──────────────────────────────────────────────────────
  uri: {
    /**
     * Register a custom URI scheme handler. The returned `Disposable`
     * is idempotent. Two plugins may not claim the same scheme — the
     * second registration's unsub is a no-op (first-match-wins).
     */
    register(
      scheme: string,
      handler: (url: URL) => void | Promise<void>,
    ): Disposable;
  };

  // ─── Activity bar ──────────────────────────────────────────────────────
  activityBar: {
    /** Add an item. `addItem` was sync in `ActivityBarAPI`; stays sync here (fire-and-forget RPC). */
    addItem(config: ActivityBarItemConfig): Disposable;
    removeItem(id: string): void;
  };

  // ─── Status bar ────────────────────────────────────────────────────────
  statusBar: {
    /**
     * Create a status-bar item. **Async** (was synchronous in
     * `StatusBarAPI`) because the host assigns the item's DOM slot
     * before returning the handle.
     */
    createItem(config: StatusBarItemConfig): Promise<StatusBarItemHandle>;
  };
}
