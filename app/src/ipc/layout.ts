// Typed wrappers for the workspace-layout Tauri commands.
//
// Layout presets are discovered dynamically via `listLayoutPresets()` —
// the set of ids is defined by the embedded + user + plugin preset files,
// not a static TypeScript union.

import { invoke } from "@tauri-apps/api/core";
import type { PresetInfo, WorkspaceLayout } from "../bindings";

export type { PresetInfo, WorkspaceLayout } from "../bindings";

export function getDefaultLayout(): Promise<WorkspaceLayout> {
  return invoke("get_default_layout");
}

export function getLayoutPreset(id: string): Promise<WorkspaceLayout> {
  return invoke("get_layout_preset", { name: id });
}

export function listLayoutPresets(): Promise<PresetInfo[]> {
  return invoke("list_layout_presets");
}
