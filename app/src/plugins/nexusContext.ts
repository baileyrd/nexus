/**
 * Host API context passed to JS plugin dispatch functions.
 *
 * Wraps Tauri invoke calls so JS plugins can access settings, emit
 * events, and call other plugins without importing Tauri directly.
 */

import { invokePluginCommand } from "../ipc/plugins";
import { getPluginSettings } from "../ipc/pluginSettings";
import { publishHostEvent } from "./events";

export interface NexusPluginContext {
  /** The plugin's reverse-DNS identifier. */
  pluginId: string;

  /** Read the plugin's current settings. */
  settings: {
    get(): Promise<Record<string, unknown>>;
  };

  /** Publish events to the kernel event bus + frontend. */
  events: {
    emit(typeId: string, payload: unknown): Promise<void>;
  };

  /** Call another plugin's IPC command. */
  ipc: {
    call(
      targetPluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<unknown>;
  };
}

export function createNexusContext(pluginId: string): NexusPluginContext {
  return {
    pluginId,
    settings: {
      get: () => getPluginSettings(pluginId),
    },
    events: {
      emit: (typeId, payload) =>
        publishHostEvent(typeId, payload as Record<string, unknown>),
    },
    ipc: {
      call: (target, cmd, args) =>
        invokePluginCommand(target, cmd, args ?? {}),
    },
  };
}
