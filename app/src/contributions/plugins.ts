// Bridges plugin-contributed palette commands into the UI contribution
// registry. Called once after `registerBuiltins()` at app boot.
//
// Each `UiContribution` surfaces as:
//   * a command handler under id `plugin:<plugin_id>:<command_id>`
//     that dispatches via `invoke_plugin_command`
//   * a palette entry pointing at that command
//
// On boot the bridge also subscribes to the Rust-side `plugins:reloaded`
// event. When a community plugin's WASM changes on disk, the backend
// drains its hot-reload queue and emits that event; the bridge disposes
// the previous snapshot of registrations and re-registers fresh ones.
//
// Failures are logged but non-fatal: a broken plugin must not prevent
// the rest of the app from booting.

import { listen } from "@tauri-apps/api/event";
import { contributions } from "./registry";
import { listPluginContributions, invokePluginCommand } from "../ipc/plugins";

type Disposable = () => void;

function warn(msg: string, err?: unknown) {
  // eslint-disable-next-line no-console
  console.warn(`[plugins] ${msg}`, err ?? "");
}

/** Disposables from the current snapshot — cleared before each resync. */
let activeDisposables: Disposable[] = [];

async function syncContributions(): Promise<void> {
  let entries;
  try {
    entries = await listPluginContributions();
  } catch (err) {
    warn("list_plugin_contributions failed — skipping plugin bridge", err);
    return;
  }

  // Drop the previous snapshot's registrations before re-registering to
  // keep the registry in sync when a plugin removes or renames commands.
  for (const dispose of activeDisposables) {
    dispose();
  }
  activeDisposables = [];

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

export async function registerPluginContributions(): Promise<void> {
  await syncContributions();

  // Re-sync whenever the backend reports hot-reloaded plugins.
  try {
    await listen<{ plugin_ids: string[] }>("plugins:reloaded", (event) => {
      // eslint-disable-next-line no-console
      console.log("[plugins] hot-reloaded:", event.payload.plugin_ids);
      void syncContributions();
    });
  } catch (err) {
    warn("failed to subscribe to plugins:reloaded — hot-reload→UI disabled", err);
  }
}
