// Typed wrappers for the workspace-layout Tauri commands.

import { invoke } from "@tauri-apps/api/core";
import type { WorkspaceLayout } from "../bindings";

export type { WorkspaceLayout } from "../bindings";

export type LayoutPresetName = "writing" | "reviewing" | "coding";

export function getDefaultLayout(): Promise<WorkspaceLayout> {
  return invoke("get_default_layout");
}

export function getLayoutPreset(
  name: LayoutPresetName,
): Promise<WorkspaceLayout> {
  return invoke("get_layout_preset", { name });
}
