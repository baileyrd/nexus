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

      <KeybindingConflicts commands={commands} />

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

/**
 * Keybinding-conflict surface (UI F-4.1.1). Scans the active palette
 * commands for any chord claimed by more than one command, surfaces
 * them as a yellow banner with the conflicting commands listed, and
 * links the user to the row they can rebind to resolve the collision.
 * Silent first-wins is what the dispatcher enforces — this panel just
 * makes the normally-hidden collision visible.
 */
function KeybindingConflicts({ commands }: { commands: PaletteCommand[] }) {
  const conflicts = useMemo(() => {
    const byChord = new Map<string, PaletteCommand[]>();
    for (const cmd of commands) {
      if (!cmd.keybinding) continue;
      const key = cmd.keybinding.trim().toLowerCase();
      const bucket = byChord.get(key) ?? [];
      bucket.push(cmd);
      byChord.set(key, bucket);
    }
    const out: Array<{ chord: string; entries: PaletteCommand[] }> = [];
    for (const [chord, entries] of byChord) {
      if (entries.length > 1) out.push({ chord, entries });
    }
    return out.sort((a, b) => a.chord.localeCompare(b.chord));
  }, [commands]);

  if (conflicts.length === 0) return null;

  return (
    <section
      className="settings-group"
      style={{
        background: "var(--color-warn-bg, #fff8e1)",
        border: "1px solid var(--color-warn, #c80)",
        padding: 12,
        borderRadius: 4,
        marginBottom: 16,
      }}
    >
      <h3 className="settings-group-title">Keybinding conflicts</h3>
      <p style={{ fontSize: "0.9em", marginTop: 0 }}>
        These chords are claimed by more than one command. The first
        registration wins — later ones are ignored silently until you
        rebind them.
      </p>
      <ul className="settings-rows" style={{ margin: 0 }}>
        {conflicts.map(({ chord, entries }) => (
          <li key={chord} className="settings-row" style={{ display: "block" }}>
            <div>
              <strong>{entries[0]?.keybinding ?? chord}</strong>
              <span style={{ marginLeft: 8, opacity: 0.7, fontSize: "0.85em" }}>
                ({entries.length} commands)
              </span>
            </div>
            <ul style={{ marginTop: 4, marginBottom: 0, paddingLeft: 20 }}>
              {entries.map((cmd, i) => (
                <li key={cmd.id} style={{ fontSize: "0.9em" }}>
                  {cmd.title}{" "}
                  <span style={{ opacity: 0.6 }}>({cmd.commandId})</span>
                  {i === 0 && (
                    <span
                      style={{
                        marginLeft: 8,
                        fontSize: "0.75em",
                        padding: "0 6px",
                        borderRadius: 10,
                        background: "var(--color-ok-bg, #dfd)",
                        color: "var(--color-ok, #393)",
                      }}
                    >
                      active
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
    </section>
  );
}
