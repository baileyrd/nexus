import { useMemo, useState } from "react";
import { usePaletteCommands, type PaletteCommand } from "../../../contributions";
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
 * Read-only hotkeys tab. Lists every registered palette command
 * grouped by category; binding UI will layer on top once manifests
 * declare default keybindings.
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
          All registered commands. Keybinding assignment is coming soon —
          for now, open the command palette with <kbd>Ctrl</kbd>+
          <kbd>K</kbd>.
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
                  <span className="settings-row-hint">
                    {cmd.keybinding ?? "unassigned"}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}
