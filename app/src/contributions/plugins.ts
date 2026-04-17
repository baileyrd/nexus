// Bridges plugin contributions into the UI contribution registry, the
// layout store, and the Settings-modal store. Called once after
// `registerBuiltins()` at app boot.
//
// What each plugin contribution becomes:
//   * `ui_command`       → a command handler + palette entry keyed by
//                          `plugin:<plugin_id>:<command_id>`
//   * `ui_panel`         → a contentType registration keyed the same
//                          way, + a `Panel` appended to the layout's
//                          left/right side panel via
//                          `useLayoutStore.setPluginPanels`
//   * `ui_settings_tab`  → pushed to `useSettingsStore.setPluginTabs`;
//                          the Settings modal renders them in the
//                          "Plugins" rail group
//   * `ui_ribbon_item`   → pushed to the layout store and merged into
//                          the workspace ribbon
//   * `ui_status_item`   → pushed to the layout store and merged into
//                          the floating status bar
//
// On boot the bridge also subscribes to the Rust-side `plugins:reloaded`
// event. When a community plugin's WASM changes on disk, the backend
// drains its hot-reload queue and emits that event; the bridge disposes
// the previous snapshot of registrations and re-registers fresh ones.
//
// Failures are logged but non-fatal: a broken plugin must not prevent
// the rest of the app from booting.

import { createElement } from "react";
import { listen } from "@tauri-apps/api/event";
import { PluginPanel } from "../components/panels/PluginPanel";
import { useLayoutStore } from "../stores/layout";
import { useSettingsStore } from "../stores/settings";
import { contributions } from "./registry";
import {
  invokePluginCommand,
  listPluginContributions,
  listPluginPanels,
  listPluginRibbonItems,
  listPluginSettingsTabs,
  listPluginSlashCommands,
  listPluginStatusItems,
  type PluginUiPanel,
  type PluginUiRibbonItem,
  type PluginUiStatusItem,
} from "../ipc/plugins";
import {
  dispatchToScript,
  evictScriptPlugin,
  refreshActivationTable,
} from "../plugins/scriptRuntime";
import {
  SLOW_COMMAND_CANCEL_MS,
  usePluginStatusStore,
} from "../plugins/status";
import { setPluginSlashCommands } from "../editor/slashCommands";

type Disposable = () => void;

function warn(msg: string, err?: unknown) {
  // eslint-disable-next-line no-console
  console.warn(`[plugins] ${msg}`, err ?? "");
}

/**
 * Wrap a plugin command invocation with the F-8.2.1 time budget + the
 * F-7.2.1 per-plugin error boundary: exceptions never escape past this
 * function, but they are recorded on the plugin-status store so the
 * Settings → Plugins panel (F-10.1.1) can surface them and offer the
 * user a disable/uninstall action. Timings are recorded on every call;
 * dispatches that cross `SLOW_COMMAND_CANCEL_MS` are also reported.
 */
async function runWithTelemetry(
  pluginId: string,
  commandId: string,
  run: () => unknown | Promise<unknown>,
): Promise<unknown> {
  const status = usePluginStatusStore.getState();
  const start = performance.now();
  try {
    const pending = Promise.resolve(run());
    const timer = new Promise<"__timeout__">((resolve) =>
      setTimeout(() => resolve("__timeout__"), SLOW_COMMAND_CANCEL_MS),
    );
    const result = await Promise.race([pending, timer]);
    const elapsed = performance.now() - start;
    status.noteCommandTiming(pluginId, commandId, elapsed);
    if (result === "__timeout__") {
      const msg = `plugin command exceeded ${SLOW_COMMAND_CANCEL_MS}ms budget`;
      status.noteError(pluginId, `${commandId}: ${msg}`);
      warn(`${pluginId}/${commandId} ${msg} — continuing in background`);
      // Let the in-flight promise settle in the background so it does
      // not leak — its eventual resolution/rejection is recorded too.
      void pending.catch((err) => status.noteError(pluginId, String(err)));
      return undefined;
    }
    return result;
  } catch (err) {
    const elapsed = performance.now() - start;
    status.noteCommandTiming(pluginId, commandId, elapsed);
    status.noteError(pluginId, `${commandId}: ${String(err)}`);
    warn(`invoke ${pluginId}/${commandId} failed`, err);
    return undefined;
  }
}

/** Disposables from the current snapshot — cleared before each resync. */
let activeDisposables: Disposable[] = [];

async function syncCommands(): Promise<void> {
  let entries;
  try {
    entries = await listPluginContributions();
  } catch (err) {
    warn("list_plugin_contributions failed — skipping", err);
    return;
  }

  for (const entry of entries) {
    // Per-plugin error boundary (UI F-7.2.1): a single broken contribution
    // must not abort registration for every plugin loaded after it.
    try {
      const commandId = `plugin:${entry.plugin_id}:${entry.command_id}`;
      const run =
        entry.runtime === "script"
          ? () => dispatchToScript(entry.plugin_id, entry.handler_id, {})
          : () => invokePluginCommand(entry.plugin_id, entry.command_id);
      const handler = async () => {
        await runWithTelemetry(entry.plugin_id, commandId, run);
      };
      activeDisposables.push(contributions.registerCommand(commandId, handler));
      activeDisposables.push(
        contributions.registerPaletteCommand({
          id: commandId,
          commandId,
          title: entry.title,
          category: entry.category ?? undefined,
          icon: entry.icon ?? undefined,
          keybinding: entry.keybinding ?? undefined,
        }),
      );
    } catch (err) {
      usePluginStatusStore
        .getState()
        .noteError(entry.plugin_id, `register ${entry.command_id}: ${String(err)}`);
      warn(`register ${entry.plugin_id}/${entry.command_id} failed`, err);
    }
  }
}

async function syncPanels(): Promise<void> {
  let panels: PluginUiPanel[];
  try {
    panels = await listPluginPanels();
  } catch (err) {
    warn("list_plugin_panels failed — skipping", err);
    useLayoutStore.getState().setPluginPanels([]);
    return;
  }

  for (const panel of panels) {
    try {
      const contentType = `plugin:${panel.plugin_id}:${panel.panel_id}`;
      const pluginId = panel.plugin_id;
      const panelId = panel.panel_id;
      activeDisposables.push(
        contributions.registerContentType(contentType, () =>
          createElement(PluginPanel, { pluginId, panelId }),
        ),
      );
    } catch (err) {
      usePluginStatusStore
        .getState()
        .noteError(panel.plugin_id, `register panel ${panel.panel_id}: ${String(err)}`);
      warn(`register panel ${panel.plugin_id}/${panel.panel_id} failed`, err);
    }
  }

  useLayoutStore.getState().setPluginPanels(panels);
}

async function syncSettingsTabs(): Promise<void> {
  try {
    const tabs = await listPluginSettingsTabs();
    useSettingsStore.getState().setPluginTabs(tabs);
  } catch (err) {
    warn("list_plugin_settings_tabs failed — skipping", err);
    useSettingsStore.getState().setPluginTabs([]);
  }
}

async function syncRibbon(): Promise<void> {
  let items: PluginUiRibbonItem[];
  try {
    items = await listPluginRibbonItems();
  } catch (err) {
    warn("list_plugin_ribbon_items failed — skipping", err);
    useLayoutStore.getState().setPluginRibbon([]);
    return;
  }
  useLayoutStore.getState().setPluginRibbon(items);
}

async function syncStatus(): Promise<void> {
  let items: PluginUiStatusItem[];
  try {
    items = await listPluginStatusItems();
  } catch (err) {
    warn("list_plugin_status_items failed — skipping", err);
    useLayoutStore.getState().setPluginStatus([]);
    return;
  }
  useLayoutStore.getState().setPluginStatus(items);
}

async function syncSlashCommands(): Promise<void> {
  try {
    const entries = await listPluginSlashCommands();
    setPluginSlashCommands(
      entries.map((e) => ({
        // Namespace plugin IDs so they don't collide with built-in IDs.
        id: `plugin:${e.plugin_id}:${e.command_id}`,
        label: e.label,
        aliases: e.aliases ?? [],
        description: e.description,
        template: e.template,
        badge: e.badge,
      })),
    );
  } catch (err) {
    warn("list_plugin_slash_commands failed — skipping", err);
    setPluginSlashCommands([]);
  }
}

async function syncAll(resetPluginIds: string[] = []): Promise<void> {
  // Drop the previous snapshot's registrations before re-registering so
  // removed/renamed plugin contributions disappear cleanly.
  for (const dispose of activeDisposables) dispose();
  activeDisposables = [];

  // Clear status entries for plugins that just hot-reloaded so their
  // "failed" / "slow" badges don't linger after a successful re-sync.
  for (const id of resetPluginIds) {
    usePluginStatusStore.getState().clearPlugin(id);
  }

  await Promise.all([
    syncCommands(),
    syncPanels(),
    syncSettingsTabs(),
    syncRibbon(),
    syncStatus(),
    syncSlashCommands(),
  ]);
}

export async function registerPluginContributions(): Promise<void> {
  await syncAll();

  // Re-sync whenever the backend reports hot-reloaded plugins.
  try {
    await listen<{ plugin_ids: string[] }>("plugins:reloaded", (event) => {
      // eslint-disable-next-line no-console
      console.log("[plugins] hot-reloaded:", event.payload.plugin_ids);
      // Evict cached script modules so they're re-loaded on next dispatch.
      for (const id of event.payload.plugin_ids) {
        evictScriptPlugin(id);
      }
      // Rebuild the activation table in case any reloaded plugin changed
      // its `[activation]` block (UI F-3.2.1).
      void refreshActivationTable();
      void syncAll(event.payload.plugin_ids);
    });
  } catch (err) {
    warn("failed to subscribe to plugins:reloaded — hot-reload→UI disabled", err);
  }
}
