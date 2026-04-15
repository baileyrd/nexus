// Typed wrappers for the layout-persistence Tauri commands.
//
// Types are declared locally rather than generated from Rust because the
// persistence file is an implementation detail of the app shell and its
// shape is unlikely to drift often — a round-trip via ts-rs/specta would
// be overhead.

import { invoke } from "@tauri-apps/api/core";

export interface PersistedLayoutState {
  leftSidePanelCollapsed: boolean;
  rightSidePanelCollapsed: boolean;
  leftActivePanelId: string | null;
  rightActivePanelId: string | null;
}

export interface ForgeUiState {
  expandedPaths: string[];
  openFile: string | null;
}

export interface LayoutPersistence {
  version: number;
  lastPresetId: string | null;
  /** Absolute path of the last opened forge. Written by the backend
   *  on every successful `open_forge`; the frontend should treat it
   *  as read-only state. */
  lastForgePath?: string | null;
  layouts: Record<string, PersistedLayoutState>;
  /** Per-forge UI state keyed by forge root absolute path. */
  forgeState?: Record<string, ForgeUiState>;
}

export function getLayoutPersistence(): Promise<LayoutPersistence> {
  return invoke("get_layout_persistence");
}

export function saveLayoutPersistence(
  state: LayoutPersistence,
): Promise<void> {
  return invoke("save_layout_persistence", { state });
}
