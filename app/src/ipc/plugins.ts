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
}

export function listPluginContributions(): Promise<PluginUiContribution[]> {
  return invoke<PluginUiContribution[]>("list_plugin_contributions");
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
