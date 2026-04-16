/**
 * Host API context passed to JS plugin dispatch functions.
 *
 * Wraps Tauri invoke calls so JS plugins can access settings, emit
 * events, and call other plugins without importing Tauri directly.
 */

import {
  contributions,
  type EditorBlockType,
  type EditorDecorationProvider,
  type EditorKeybinding,
  type TreeDataProvider,
} from "../contributions";
import { invokePluginCommand } from "../ipc/plugins";
import { getPluginSettings } from "../ipc/pluginSettings";
import { publishHostEvent } from "./events";
import { useToastStore, type ToastLevel } from "../stores/toast";

/** Minimal disposable contract mirroring the contribution registry. */
export type Disposable = () => void;

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

  /**
   * Editor-surface extension points (PRD-08 §14.1–14.3). Plugins hold
   * onto the returned disposables and call them from `onStop` so their
   * contributions are removed when the plugin is unloaded.
   */
  editor: {
    registerBlockType(type: EditorBlockType): Disposable;
    registerDecorationProvider(
      provider: EditorDecorationProvider,
    ): Disposable;
    registerKeybinding(binding: EditorKeybinding): Disposable;
  };

  /**
   * Host UI APIs. Lets plugins surface feedback to the user (toasts,
   * future: quick-pick dialogs, input prompts) without importing
   * Tauri directly or writing bespoke React components.
   */
  ui: {
    /**
     * Show an in-app toast notification. Auto-dismissed after ~5 s.
     * `level` controls the colour badge: "info" (default), "warn", or "error".
     */
    notify(level: ToastLevel, message: string): void;

    /**
     * Register a tree-data provider and claim the content-type `viewId`.
     * A generic tree panel is automatically wired up, so the plugin
     * doesn't need to ship a bespoke React component.
     * Returns a disposable that un-registers on plugin stop.
     */
    registerTreeDataProvider(viewId: string, provider: TreeDataProvider): Disposable;
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
    editor: {
      registerBlockType: (type) =>
        contributions.registerEditorBlockType(type),
      registerDecorationProvider: (provider) =>
        contributions.registerEditorDecorationProvider(provider),
      registerKeybinding: (binding) =>
        contributions.registerEditorKeybinding(binding),
    },
    ui: {
      notify: (level, message) => {
        useToastStore.getState().add({ level, message, source: pluginId });
      },
      registerTreeDataProvider: (viewId, provider) =>
        contributions.registerTreeDataProvider(viewId, provider),
    },
  };
}
