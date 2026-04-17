// Typed wrappers for the nexus-app plugin Tauri commands.
//
// Mirrors `nexus_plugins::UiContribution`. Kept as a hand-written structural
// type (instead of a generated binding) because the plugin crate doesn't emit
// ts-rs types today and the shape is small.

import { invoke } from "@tauri-apps/api/core";

export interface PluginUiContribution {
  plugin_id: string;
  command_id: string;
  handler_id: number;
  runtime: string;
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
  handler_id: number;
  runtime: string;
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
  runtime: string;
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
 * Mirrors `nexus_plugins::UiStatusItemContribution`. `command_id` is
 * pre-qualified when set; `null` means the entry is a plain counter.
 */
export interface PluginUiStatusItem {
  plugin_id: string;
  status_id: string;
  text: string | null;
  icon: string | null;
  tooltip: string | null;
  command_id: string | null;
}

/**
 * Mirrors `nexus_plugins::UiSlashCommandContribution`. A plugin-
 * contributed entry for the editor's `/` trigger overlay.
 */
export interface PluginUiSlashCommand {
  plugin_id: string;
  command_id: string;
  label: string;
  description: string;
  aliases: string[];
  badge: string;
  template: string;
}

/**
 * Mirrors `nexus_app::plugins::PluginSummary`. Trust level is `"core"`
 * or `"community"`; status is `"loaded"`, `"initialized"`, `"running"`,
 * `"stopped"`, or `"crashed"`.
 */
export interface SubscriptionSummary {
  id: string;
  filter: string;
  enabled: boolean;
}

export interface PluginSummary {
  id: string;
  name: string;
  version: string;
  trust_level: string;
  status: string;
  runtime: string;
  event_subscriptions: SubscriptionSummary[];
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

export function listPluginStatusItems(): Promise<PluginUiStatusItem[]> {
  return invoke<PluginUiStatusItem[]>("list_plugin_status_items");
}

export function listPluginSlashCommands(): Promise<PluginUiSlashCommand[]> {
  return invoke<PluginUiSlashCommand[]>("list_plugin_slash_commands");
}

export function listPlugins(): Promise<PluginSummary[]> {
  return invoke<PluginSummary[]>("list_plugins");
}

/**
 * Per-plugin activation triggers (UI F-3.2.1). Each entry applies to a
 * script plugin that declared `[activation]` in its manifest. WASM and
 * eager plugins are omitted from the response.
 */
export interface PluginActivation {
  plugin_id: string;
  on_command: string[];
  on_content_type: string[];
  on_uri_scheme: string[];
}

export function listPluginActivations(): Promise<PluginActivation[]> {
  return invoke<PluginActivation[]>("list_plugin_activations");
}

/**
 * Declared capability strings for a plugin (UI F-2.2.1). `required` +
 * `optional` together form the set of caps the plugin may use. Surfaces
 * on `NexusPluginContext` cross-check their backing capability against
 * this set before each call and warn when an undeclared cap is used.
 */
export interface PluginCapabilities {
  plugin_id: string;
  required: string[];
  optional: string[];
}

export function listPluginCapabilities(): Promise<PluginCapabilities[]> {
  return invoke<PluginCapabilities[]>("list_plugin_capabilities");
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

/**
 * Read the JS source code for a script plugin.
 */
export function readPluginScript(pluginId: string): Promise<string> {
  return invoke<string>("read_plugin_script", { pluginId });
}

/**
 * Toggle an event subscription on or off for a plugin.
 */
export function togglePluginSubscription(
  pluginId: string,
  subscriptionId: string,
  enabled: boolean,
): Promise<void> {
  return invoke<void>("toggle_plugin_subscription", {
    pluginId,
    subscriptionId,
    enabled,
  });
}

/**
 * Dispatch a capability-checked plugin-to-plugin IPC call.
 *
 * Verifies that `callerPluginId` holds the `IpcCall` capability before
 * dispatching to `targetPluginId`. Use this when one plugin's UI needs
 * to invoke another plugin's command.
 */
export function invokePluginIpc(
  callerPluginId: string,
  targetPluginId: string,
  commandId: string,
  args: unknown = {},
): Promise<unknown> {
  return invoke<unknown>("invoke_plugin_ipc", {
    callerPluginId,
    targetPluginId,
    commandId,
    args,
  });
}
