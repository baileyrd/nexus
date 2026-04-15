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
  type PluginUiPanel,
  type PluginUiRibbonItem,
} from "../ipc/plugins";

type Disposable = () => void;

function warn(msg: string, err?: unknown) {
  // eslint-disable-next-line no-console
  console.warn(`[plugins] ${msg}`, err ?? "");
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
    const commandId = `plugin:${entry.plugin_id}:${entry.command_id}`;
    activeDisposables.push(
      contributions.registerCommand(commandId, async () => {
        try {
          const result = await invokePluginCommand(entry.plugin_id, entry.command_id);
          // eslint-disable-next-line no-console
          console.log(`[plugin:${entry.plugin_id}] ${entry.command_id} →`, result);
        } catch (err) {
          warn(`invoke ${entry.plugin_id}/${entry.command_id} failed`, err);
        }
      }),
    );
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
    const contentType = `plugin:${panel.plugin_id}:${panel.panel_id}`;
    const pluginId = panel.plugin_id;
    const panelId = panel.panel_id;
    activeDisposables.push(
      contributions.registerContentType(contentType, () =>
        createElement(PluginPanel, { pluginId, panelId }),
      ),
    );
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

async function syncAll(): Promise<void> {
  // Drop the previous snapshot's registrations before re-registering so
  // removed/renamed plugin contributions disappear cleanly.
  for (const dispose of activeDisposables) dispose();
  activeDisposables = [];

  await Promise.all([
    syncCommands(),
    syncPanels(),
    syncSettingsTabs(),
    syncRibbon(),
  ]);
}

export async function registerPluginContributions(): Promise<void> {
  await syncAll();

  // Re-sync whenever the backend reports hot-reloaded plugins.
  try {
    await listen<{ plugin_ids: string[] }>("plugins:reloaded", (event) => {
      // eslint-disable-next-line no-console
      console.log("[plugins] hot-reloaded:", event.payload.plugin_ids);
      void syncAll();
    });
  } catch (err) {
    warn("failed to subscribe to plugins:reloaded — hot-reload→UI disabled", err);
  }
}
