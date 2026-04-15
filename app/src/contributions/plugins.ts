// Bridges plugin-contributed palette commands into the UI contribution
// registry. Called once after `registerBuiltins()` at app boot.
//
// Each `UiContribution` surfaces as:
//   * a command handler under id `plugin:<plugin_id>:<command_id>`
//     that dispatches via `invoke_plugin_command`
//   * a palette entry pointing at that command
//
// Failures are logged but non-fatal: a broken plugin must not prevent
// the rest of the app from booting.

import { contributions } from "./registry";
import { listPluginContributions, invokePluginCommand } from "../ipc/plugins";

function warn(msg: string, err?: unknown) {
  // eslint-disable-next-line no-console
  console.warn(`[plugins] ${msg}`, err ?? "");
}

export async function registerPluginContributions(): Promise<void> {
  let entries;
  try {
    entries = await listPluginContributions();
  } catch (err) {
    warn("list_plugin_contributions failed — skipping plugin bridge", err);
    return;
  }

  for (const entry of entries) {
    const commandId = `plugin:${entry.plugin_id}:${entry.command_id}`;
    contributions.registerCommand(commandId, async () => {
      try {
        const result = await invokePluginCommand(entry.plugin_id, entry.command_id);
        // eslint-disable-next-line no-console
        console.log(`[plugin:${entry.plugin_id}] ${entry.command_id} →`, result);
      } catch (err) {
        warn(`invoke ${entry.plugin_id}/${entry.command_id} failed`, err);
      }
    });
    contributions.registerPaletteCommand({
      id: commandId,
      commandId,
      title: entry.title,
      category: entry.category ?? undefined,
      icon: entry.icon ?? undefined,
    });
  }
}
