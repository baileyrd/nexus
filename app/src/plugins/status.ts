/**
 * Plugin status store (UI F-7.2.1 / UI F-10.1.1 / UI F-10.3.1).
 *
 * Tracks per-plugin health so the shell can surface failed / slow plugins
 * in the forthcoming "Show Running Extensions" settings tab. Entries are
 * written by:
 *  - the contribution bridge (`contributions/plugins.ts`) on successful
 *    syncs and lifecycle failures,
 *  - the script runtime (`plugins/scriptRuntime.ts`) on dispatch,
 *  - the plugin command dispatcher (`contributions/registry.ts` invoke
 *    wrapper) on slow / failing commands.
 *
 * Reads are via a Zustand store so any React component can subscribe.
 * Writes are idempotent — callers don't have to check whether an entry
 * exists first.
 */

import { create } from "zustand";

export type PluginStatusLevel = "ok" | "slow" | "failed";

export interface PluginStatusEntry {
  pluginId: string;
  level: PluginStatusLevel;
  /** Most recent error message, if any. */
  lastError?: string;
  /** Unix millis of the most recent error. */
  lastErrorAt?: number;
  /** Longest observed command dispatch time in ms. */
  slowestCommandMs?: number;
  /** Command id that produced the slowest observed dispatch. */
  slowestCommandId?: string;
  /** Lifecycle-hook timings in ms: `onInit`, `onStart`. */
  lifecycleMs?: { onInit?: number; onStart?: number };
}

interface StatusState {
  entries: Record<string, PluginStatusEntry>;
  noteError(pluginId: string, message: string): void;
  noteCommandTiming(pluginId: string, commandId: string, durationMs: number): void;
  noteLifecycle(
    pluginId: string,
    hook: "onInit" | "onStart",
    durationMs: number,
  ): void;
  /** Reset entries for a plugin (e.g. on hot-reload). */
  clearPlugin(pluginId: string): void;
  list(): PluginStatusEntry[];
}

function ensure(state: StatusState, pluginId: string): PluginStatusEntry {
  return (
    state.entries[pluginId] ?? {
      pluginId,
      level: "ok",
    }
  );
}

/**
 * A single dispatch slower than this threshold marks the plugin as
 * "slow" (UI F-8.2.1). 250 ms is the user-perceptible threshold that
 * starts to feel laggy inside a keyboard-driven palette flow.
 */
export const SLOW_COMMAND_WARN_MS = 250;

/**
 * Hard timeout (UI F-8.2.1). Plugin commands still running after this
 * mark are reported as failed in the status panel; the dispatch itself
 * cannot be cancelled (JS runtime limitation for script plugins) but the
 * slow-plugin telemetry is recorded so the user can disable the plugin.
 */
export const SLOW_COMMAND_CANCEL_MS = 2_000;

export const usePluginStatusStore = create<StatusState>((set, get) => ({
  entries: {},

  noteError(pluginId, message) {
    set((state) => ({
      entries: {
        ...state.entries,
        [pluginId]: {
          ...ensure(state, pluginId),
          level: "failed",
          lastError: message,
          lastErrorAt: Date.now(),
        },
      },
    }));
  },

  noteCommandTiming(pluginId, commandId, durationMs) {
    set((state) => {
      const prev = ensure(state, pluginId);
      const prevSlowest = prev.slowestCommandMs ?? 0;
      const nextSlowest =
        durationMs > prevSlowest ? durationMs : prevSlowest;
      const nextSlowestId =
        durationMs > prevSlowest ? commandId : prev.slowestCommandId;
      const nextLevel: PluginStatusLevel =
        prev.level === "failed"
          ? "failed"
          : durationMs >= SLOW_COMMAND_WARN_MS
            ? "slow"
            : prev.level;
      return {
        entries: {
          ...state.entries,
          [pluginId]: {
            ...prev,
            level: nextLevel,
            slowestCommandMs: nextSlowest,
            slowestCommandId: nextSlowestId,
          },
        },
      };
    });
  },

  noteLifecycle(pluginId, hook, durationMs) {
    set((state) => {
      const prev = ensure(state, pluginId);
      return {
        entries: {
          ...state.entries,
          [pluginId]: {
            ...prev,
            lifecycleMs: { ...(prev.lifecycleMs ?? {}), [hook]: durationMs },
          },
        },
      };
    });
  },

  clearPlugin(pluginId) {
    set((state) => {
      const { [pluginId]: _gone, ...rest } = state.entries;
      return { entries: rest };
    });
  },

  list() {
    return Object.values(get().entries).sort((a, b) =>
      a.pluginId.localeCompare(b.pluginId),
    );
  },
}));
