// Typed wrappers for per-plugin JSON-schema settings. The backend
// owns parsing / validation / persistence; the frontend only renders
// a form and ships an updated settings object back.

import { invoke } from "@tauri-apps/api/core";

/** JSON Schema shape, kept as `unknown` here — the form renderer
 *  narrows this per-field rather than imposing a full schema type. */
export type JsonSchema = Record<string, unknown>;

export function getPluginSettingsSchema(pluginId: string): Promise<JsonSchema | null> {
  return invoke<JsonSchema | null>("get_plugin_settings_schema", { pluginId });
}

export function getPluginSettings(pluginId: string): Promise<Record<string, unknown>> {
  return invoke<Record<string, unknown>>("get_plugin_settings", { pluginId });
}

export function savePluginSettings(
  pluginId: string,
  settings: Record<string, unknown>,
): Promise<void> {
  return invoke<void>("save_plugin_settings", { pluginId, settings });
}
