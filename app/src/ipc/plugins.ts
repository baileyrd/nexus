// Typed wrappers for the nexus-app plugin Tauri commands.
//
// Mirrors `nexus_plugins::UiContribution`. Kept as a hand-written structural
// type (instead of a generated binding) because the plugin crate doesn't emit
// ts-rs types today and the shape is small.

import { invoke } from "@tauri-apps/api/core";

export interface PluginUiContribution {
  plugin_id: string;
  command_id: string;
  title: string;
  category: string | null;
  icon: string | null;
  keybinding: string | null;
}

/**
 * Mirrors `nexus_plugins::UiPanelContribution`. `side` is `"left"` or
 * `"right"`; the frontend merges these into the active layout's side
 * panels at render time.
 */
export interface PluginUiPanel {
  plugin_id: string;
  panel_id: string;
  title: string;
  icon: string;
  side: string;
}

/**
 * Mirrors `nexus_plugins::UiSettingsTabContribution`. The Settings
 * modal renders one row per tab under its "Plugins" rail group.
 */
export interface PluginUiSettingsTab {
  plugin_id: string;
  plugin_name: string;
  plugin_version: string;
  tab_id: string;
  title: string;
  icon: string;
}

/**
 * Mirrors `nexus_plugins::UiRibbonItemContribution`. `command_id` is
 * pre-qualified (`plugin:<plugin_id>:<command_id>`) so the layout
 * merge can pass it straight to `contributions.invokeCommand`.
 */
export interface PluginUiRibbonItem {
  plugin_id: string;
  ribbon_id: string;
  icon: string;
  tooltip: string;
  command_id: string;
}

/**
 * Mirrors `nexus_app::plugins::PluginSummary`. Trust level is `"core"`
 * or `"community"`; status is `"loaded"`, `"initialized"`, `"running"`,
 * `"stopped"`, or `"crashed"`.
 */
export interface PluginSummary {
  id: string;
  name: string;
  version: string;
  trust_level: string;
  status: string;
}

export function listPluginContributions(): Promise<PluginUiContribution[]> {
  return invoke<PluginUiContribution[]>("list_plugin_contributions");
}

export function listPluginPanels(): Promise<PluginUiPanel[]> {
  return invoke<PluginUiPanel[]>("list_plugin_panels");
}

export function listPluginSettingsTabs(): Promise<PluginUiSettingsTab[]> {
  return invoke<PluginUiSettingsTab[]>("list_plugin_settings_tabs");
}

export function listPluginRibbonItems(): Promise<PluginUiRibbonItem[]> {
  return invoke<PluginUiRibbonItem[]>("list_plugin_ribbon_items");
}

export function listPlugins(): Promise<PluginSummary[]> {
  return invoke<PluginSummary[]>("list_plugins");
}

export function invokePluginCommand(
  pluginId: string,
  commandId: string,
  args: unknown = {},
): Promise<unknown> {
  return invoke<unknown>("invoke_plugin_command", {
    pluginId,
    commandId,
    args,
  });
}
