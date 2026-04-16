/**
 * JS plugin runtime: loads script plugins as ES modules and dispatches
 * handler calls locally in the WebView (no IPC round-trip to the backend).
 */

import { readPluginScript } from "../ipc/plugins";
import { createNexusContext, type NexusPluginContext } from "./nexusContext";

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

/** Cache of loaded script plugin modules. */
const loaded = new Map<string, ScriptPlugin>();

/**
 * Load a script plugin by reading its source from the backend and
 * evaluating it as an ES module via a Blob URL dynamic import.
 */
export async function loadScriptPlugin(
  pluginId: string,
): Promise<ScriptPlugin> {
  const cached = loaded.get(pluginId);
  if (cached) return cached;

  const source = await readPluginScript(pluginId);

  // Create a Blob URL so dynamic import() works without a custom protocol.
  const blob = new Blob([source], { type: "application/javascript" });
  const url = URL.createObjectURL(blob);
  try {
    const mod = (await import(/* @vite-ignore */ url)) as ScriptPlugin;
    loaded.set(pluginId, mod);

    // Call lifecycle hooks if declared.
    const ctx = createNexusContext(pluginId);
    if (mod.onInit) await mod.onInit(ctx);
    if (mod.onStart) await mod.onStart(ctx);

    return mod;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Dispatch a handler call to a loaded script plugin.
 * The module is loaded on first call and cached thereafter.
 */
export async function dispatchToScript(
  pluginId: string,
  handlerId: number,
  args: unknown,
): Promise<unknown> {
  const plugin = await loadScriptPlugin(pluginId);
  const ctx = createNexusContext(pluginId);
  return plugin.dispatch(handlerId, args, ctx);
}

/**
 * Evict a plugin from the cache (e.g. on hot-reload).
 */
export function evictScriptPlugin(pluginId: string): void {
  loaded.delete(pluginId);
}
