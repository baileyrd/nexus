// Typed wrappers for the nexus-app keybinding-overrides Tauri commands.

import { invoke } from "@tauri-apps/api/core";

export interface KeybindingOverrides {
  version: number;
  /** `commandId` → overriding chord. Absent keys inherit the manifest default. */
  overrides: Record<string, string>;
}

export function getKeybindingOverrides(): Promise<KeybindingOverrides> {
  return invoke<KeybindingOverrides>("get_keybinding_overrides");
}

export function setKeybindingOverride(
  commandId: string,
  binding: string,
): Promise<void> {
  return invoke<void>("set_keybinding_override", { commandId, binding });
}

export function clearKeybindingOverride(commandId: string): Promise<void> {
  return invoke<void>("clear_keybinding_override", { commandId });
}
