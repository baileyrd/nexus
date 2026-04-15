import { useMemo, useState } from "react";
import { contributions, usePaletteCommands, type PaletteCommand } from "../../../contributions";
import { KeyCaptureInput } from "../../../keybindings/KeyCaptureInput";
import { resetOverride, saveOverride } from "../../../keybindings/overrides";
import { Icon } from "../../Icon";

type Grouped = Array<[string, PaletteCommand[]]>;

function group(commands: PaletteCommand[], query: string): Grouped {
  const q = query.trim().toLowerCase();
  const filtered = q
    ? commands.filter(
        (c) =>
          c.title.toLowerCase().includes(q) ||
          (c.category ?? "").toLowerCase().includes(q),
      )
    : commands;
  const buckets = new Map<string, PaletteCommand[]>();
  for (const c of filtered) {
    const key = c.category ?? "Uncategorised";
    const bucket = buckets.get(key) ?? [];
    bucket.push(c);
    buckets.set(key, bucket);
  }
  return Array.from(buckets.entries()).sort(([a], [b]) => a.localeCompare(b));
}

/**
 * Hotkeys tab. Lists every registered palette command grouped by
 * category. Each row shows the effective binding (manifest default
 * or user override) and lets the user capture a new chord or reset
 * to default.
 */
export function HotkeysTab() {
  const commands = usePaletteCommands();
  const [query, setQuery] = useState("");
  const grouped = useMemo(() => group(commands, query), [commands, query]);

  return (
    <div className="settings-tab">
      <header className="settings-section-header">
        <h2>Hotkeys</h2>
        <p className="settings-section-desc">
          Click a binding to record a new chord. Your overrides persist
          across restarts; the × resets to the manifest default.
        </p>
      </header>

      <input
        className="settings-filter"
        type="search"
        placeholder="Filter commands…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      {grouped.length === 0 ? (
        <p className="settings-empty">No commands match.</p>
      ) : (
        grouped.map(([category, items]) => (
          <section key={category} className="settings-group">
            <h3 className="settings-group-title">{category}</h3>
            <ul className="settings-rows">
              {items.map((cmd) => (
                <li key={cmd.id} className="settings-row">
                  <div className="settings-row-body">
                    {cmd.icon && (
                      <Icon name={cmd.icon} size={16} className="settings-row-icon" />
                    )}
                    <span className="settings-row-title">{cmd.title}</span>
                  </div>
                  <KeyCaptureInput
                    value={cmd.keybinding}
                    hasOverride={
                      contributions.getKeybindingOverride(cmd.commandId) !== undefined
                    }
                    onCommit={(binding) => {
                      void saveOverride(cmd.commandId, binding);
                    }}
                    onReset={() => {
                      void resetOverride(cmd.commandId);
                    }}
                  />
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}
