/**
 * Default-export shape for a sandboxed community plugin (WI-30c).
 *
 * Mirrors the first-party {@link import('../index').ScriptPlugin} but
 * collapsed to a two-hook lifecycle — sandboxed plugins have no
 * `dispatch` / `onInit` / `onStart` split because there's no kernel
 * handler dispatch and no separate init-vs-start phase across the
 * sandbox boundary.
 */

import type { SandboxedPluginContext } from './context';

export interface SandboxedPlugin {
  /**
   * Called once, after the guest completes the handshake with the
   * host. The plugin receives its sandbox-scoped context and is free
   * to register commands, panels, status-bar items, and so on.
   *
   * All registrations made through `ctx.*.register` / `ctx.*.on` are
   * auto-disposed by the host when the plugin unloads — do **not**
   * track them manually for cleanup.
   */
  activate(ctx: SandboxedPluginContext): void | Promise<void>;

  /**
   * Called before the iframe is torn down. Clean up plugin-owned
   * state — timers, in-flight fetches, WebSocket handles, decoded
   * binary buffers — anything the host can't sweep for you.
   *
   * Subscriptions registered via `ctx.*.register` / `ctx.*.on` are
   * disposed automatically by the host before this hook runs; this
   * hook should not re-dispose them.
   */
  deactivate?(): void | Promise<void>;
}
