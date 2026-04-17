/**
 * JS plugin runtime: loads script plugins as ES modules and dispatches
 * handler calls locally in the WebView (no IPC round-trip to the backend).
 */

import { readPluginScript } from "../ipc/plugins";
import {
  createNexusContext,
  type DisposableStore,
  type NexusPluginContext,
} from "./nexusContext";
import { usePluginStatusStore } from "./status";

/** The contract a JS plugin module must satisfy. */
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

interface LoadedPlugin {
  module: ScriptPlugin;
  ctx: NexusPluginContext;
  store: DisposableStore;
}

/** Cache of loaded script plugin modules keyed by plugin id. */
const loaded = new Map<string, LoadedPlugin>();

/**
 * Load a script plugin by reading its source from the backend and
 * evaluating it as an ES module via a Blob URL dynamic import.
 */
export async function loadScriptPlugin(
  pluginId: string,
): Promise<ScriptPlugin> {
  const cached = loaded.get(pluginId);
  if (cached) return cached.module;

  const source = await readPluginScript(pluginId);

  // Create a Blob URL so dynamic import() works without a custom protocol.
  const blob = new Blob([source], { type: "application/javascript" });
  const url = URL.createObjectURL(blob);
  try {
    const mod = (await import(/* @vite-ignore */ url)) as ScriptPlugin;
    const ctx = createNexusContext(pluginId);
    loaded.set(pluginId, { module: mod, ctx, store: ctx.disposables });

    // Call lifecycle hooks with a per-plugin error boundary (UI F-7.2.1)
    // and performance.measure instrumentation (UI F-10.3.1). A failing
    // hook marks the plugin "failed" on the status store but never
    // bubbles — registration for subsequent plugins must proceed.
    await runLifecycle(pluginId, "onInit", () => mod.onInit?.(ctx));
    await runLifecycle(pluginId, "onStart", () => mod.onStart?.(ctx));

    return mod;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Dispatch a handler call to a loaded script plugin.
 * The module is loaded on first call and cached thereafter. Re-uses the
 * ctx + disposable store allocated at load time so `register*` calls from
 * dispatch handlers are tracked for flush on stop.
 */
export async function dispatchToScript(
  pluginId: string,
  handlerId: number,
  args: unknown,
): Promise<unknown> {
  const plugin = await loadScriptPlugin(pluginId);
  const entry = loaded.get(pluginId)!;
  return plugin.dispatch(handlerId, args, entry.ctx);
}

/**
 * Shared lifecycle-hook runner. Wraps `onInit` / `onStart` / `onStop`
 * in a try/catch that records failures on the plugin-status store
 * (UI F-7.2.1) and brackets the call with `performance.mark` /
 * `performance.measure` so the cold-start cost of each plugin is visible
 * in DevTools and in the future "Show Running Extensions" panel
 * (UI F-10.3.1).
 */
async function runLifecycle(
  pluginId: string,
  hook: "onInit" | "onStart" | "onStop",
  fn: () => void | Promise<void> | undefined,
): Promise<void> {
  const mark = `plugin:${pluginId}:${hook}`;
  const start = performance.now();
  try {
    performance.mark(`${mark}:start`);
    await fn();
  } catch (err) {
    usePluginStatusStore.getState().noteError(pluginId, `${hook}: ${String(err)}`);
    // eslint-disable-next-line no-console
    console.warn(`[scriptRuntime] ${pluginId} ${hook} threw: ${String(err)}`);
  } finally {
    try {
      performance.mark(`${mark}:end`);
      performance.measure(mark, `${mark}:start`, `${mark}:end`);
    } catch {
      // Some browsers throw if the start mark is missing — ignore.
    }
    const duration = performance.now() - start;
    if (hook !== "onStop") {
      usePluginStatusStore.getState().noteLifecycle(pluginId, hook, duration);
    }
  }
}

/**
 * Stop a single loaded plugin — run `onStop` (best-effort) and flush its
 * disposable store. Used by the shell on window close and by hot-reload
 * before it re-imports the module.
 */
export async function stopScriptPlugin(pluginId: string): Promise<void> {
  const entry = loaded.get(pluginId);
  if (!entry) return;
  await runLifecycle(pluginId, "onStop", () => entry.module.onStop?.(entry.ctx));
  entry.store.dispose();
}

/**
 * Evict a plugin from the cache (e.g. on hot-reload). Does not call
 * `onStop` — callers that need graceful shutdown should await
 * `stopScriptPlugin` first.
 */
export function evictScriptPlugin(pluginId: string): void {
  loaded.delete(pluginId);
}

/**
 * Iterate every currently-loaded plugin id. Used by the shell's
 * `beforeunload` handler (UI F-7.3.1) to stop plugins on window close.
 */
export function loadedScriptPluginIds(): string[] {
  return Array.from(loaded.keys());
}

/**
 * Best-effort stop of every loaded plugin. Invoked from the shell's
 * `beforeunload` listener so plugins get a chance to flush state before
 * the WebView tears down. Runs with a short per-plugin budget so a
 * blocked plugin cannot delay window close indefinitely.
 */
export async function stopAllScriptPlugins(
  perPluginBudgetMs = 100,
): Promise<void> {
  const ids = loadedScriptPluginIds();
  await Promise.all(
    ids.map((id) =>
      Promise.race([
        stopScriptPlugin(id),
        new Promise<void>((resolve) =>
          setTimeout(resolve, perPluginBudgetMs),
        ),
      ]),
    ),
  );
}
