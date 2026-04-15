// Plugin → frontend event bus.
//
// Each plugin handler can surface side-channel events by returning
// `{ events: [{ topic, payload }, …] }` as part of its JSON response.
// The Rust side (`invoke_plugin_command`) pulls the `events` array off
// and emits one `plugin:event` Tauri event per entry with envelope
// `{ plugin_id, topic, payload }`.
//
// Frontend consumers subscribe via `onPluginEvent(topic, handler)` —
// a thin wrapper that listens for `plugin:event` and dispatches by
// exact topic match. A module-level console logger is attached at
// boot so `[plugin:event]` lines always appear in DevTools.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

/** Reverse-DNS prefix reserved for events published by the host shell. */
export const HOST_EVENT_SOURCE = "nexus.host";

/** Well-known host-origin topics. Plugins subscribe to these via
 *  `filter = "nexus.host.*"` or one of these exact strings in the
 *  manifest's `[[registrations.event_subscriber]]` block. */
export const HostTopics = {
  forgeSwitched: `${HOST_EVENT_SOURCE}.forge_switched`,
  fileOpened: `${HOST_EVENT_SOURCE}.file_opened`,
  fileClosed: `${HOST_EVENT_SOURCE}.file_closed`,
  themeChanged: `${HOST_EVENT_SOURCE}.theme_changed`,
} as const;

/** Envelope delivered on every `plugin:event` Tauri event. */
export interface PluginEvent<T = unknown> {
  plugin_id: string;
  topic: string;
  payload: T;
}

type Handler<T> = (event: PluginEvent<T>) => void;

/**
 * Subscribe to plugin events matching `topic` exactly.
 *
 * Returns a promise for an unlisten function — await it if you need
 * the listener to be active before proceeding. The promise resolves
 * as soon as the Tauri channel is wired up.
 */
export function onPluginEvent<T = unknown>(
  topic: string,
  handler: Handler<T>,
): Promise<UnlistenFn> {
  return listen<PluginEvent<T>>("plugin:event", (event) => {
    if (event.payload.topic === topic) handler(event.payload);
  });
}

/**
 * Attach a module-level console logger. Called once at app boot so
 * every plugin event surfaces in DevTools with a consistent prefix,
 * even when no component is actively listening.
 */
export async function startPluginEventLogger(): Promise<void> {
  try {
    await listen<PluginEvent>("plugin:event", (event) => {
      // eslint-disable-next-line no-console
      console.log(
        `[plugin:event] ${event.payload.plugin_id} · ${event.payload.topic}`,
        event.payload.payload,
      );
    });
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("[plugins] failed to subscribe to plugin:event channel", err);
  }
}

/**
 * Publish a host-origin lifecycle event onto the kernel event bus.
 * The backend fans it out to every plugin subscribed via
 * `[[registrations.event_subscriber]]`. Failures are logged and
 * swallowed — lifecycle events are best-effort from the UI's POV
 * (e.g. a forge switch still succeeds even if no plugins are
 * listening).
 *
 * `topic` must begin with `"nexus.host."`; prefer the constants
 * exported as {@link HostTopics}.
 */
export async function publishHostEvent(
  topic: string,
  payload: unknown = null,
): Promise<void> {
  try {
    await invoke("publish_host_event", { topic, payload });
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn(`[plugins] publish_host_event(${topic}) failed`, err);
  }
}
