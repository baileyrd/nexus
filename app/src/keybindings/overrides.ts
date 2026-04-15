// User keybinding overrides — glue between the Rust persistence file
// and the in-memory contribution registry.
//
// Overrides are layered on top of manifest defaults: the stored
// `PaletteCommand.keybinding` is left untouched; consumers see the
// merged effective binding via `listPaletteCommands`.

import { contributions } from "../contributions/registry";
import {
  clearKeybindingOverride as ipcClearOverride,
  getKeybindingOverrides as ipcGetOverrides,
  setKeybindingOverride as ipcSetOverride,
} from "../ipc/keybindings";

function warn(msg: string, err?: unknown) {
  // eslint-disable-next-line no-console
  console.warn(`[keybindings] ${msg}`, err ?? "");
}

/**
 * Pull persisted overrides from disk and seed the registry. Called
 * once at boot. Failures are logged but non-fatal — missing or
 * corrupt overrides file just means "no user customisations yet".
 */
export async function hydrateOverrides(): Promise<void> {
  try {
    const state = await ipcGetOverrides();
    contributions.hydrateKeybindingOverrides(state.overrides);
  } catch (err) {
    warn("failed to hydrate keybinding overrides", err);
  }
}

/** Persist `binding` as the override for `commandId`. */
export async function saveOverride(
  commandId: string,
  binding: string,
): Promise<void> {
  contributions.setKeybindingOverride(commandId, binding);
  try {
    await ipcSetOverride(commandId, binding);
  } catch (err) {
    warn(`failed to persist override for ${commandId}`, err);
  }
}

/** Remove the override for `commandId`; the manifest default takes over. */
export async function resetOverride(commandId: string): Promise<void> {
  contributions.clearKeybindingOverride(commandId);
  try {
    await ipcClearOverride(commandId);
  } catch (err) {
    warn(`failed to clear override for ${commandId}`, err);
  }
}
