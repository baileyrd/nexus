import { useEffect, useMemo } from "react";
import { contributions, usePaletteCommands } from "../contributions";
import { isCapturing } from "./capture-state";
import { matchesBinding, parseKeybinding, type ParsedBinding } from "./parse";

interface CompiledBinding {
  commandId: string;
  binding: ParsedBinding;
  raw: string;
}

/**
 * Listens for global keydown events and dispatches the matching
 * registered palette command via `contributions.invokeCommand`.
 *
 * Reads from the live palette-command registry (plugin contributions
 * + builtins), so hot-reloading a plugin instantly re-binds its
 * shortcuts without restart.
 *
 * Mounted once at the app root; renders nothing. Binds a single
 * capture-phase keydown listener. Precedence on a conflict is "first
 * registration wins" — since the builtins (`Mod+K` palette toggle,
 * `Mod+,` Settings) register at app boot before any plugin, a
 * plugin trying to take the same chord silently loses.
 */
export function KeybindingDispatcher() {
  const commands = usePaletteCommands();

  const compiled = useMemo<CompiledBinding[]>(() => {
    const out: CompiledBinding[] = [];
    for (const cmd of commands) {
      if (!cmd.keybinding) continue;
      const parsed = parseKeybinding(cmd.keybinding);
      if (!parsed) {
        // eslint-disable-next-line no-console
        console.warn(
          `[keybindings] unparseable binding "${cmd.keybinding}" on ${cmd.commandId} — ignored`,
        );
        continue;
      }
      out.push({ commandId: cmd.commandId, binding: parsed, raw: cmd.keybinding });
    }
    return out;
  }, [commands]);

  useEffect(() => {
    if (compiled.length === 0) return undefined;

    function handler(e: KeyboardEvent) {
      // Silent while the Hotkeys tab is recording a new chord — we
      // must not fire an existing command when the user is trying to
      // rebind it.
      if (isCapturing()) return;

      // Skip when typing in an editable surface, unless the chord
      // includes Ctrl/Cmd/Alt — pure letter shortcuts inside an input
      // would swallow the keystroke.
      const target = e.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.isContentEditable === true;

      for (const c of compiled) {
        if (!matchesBinding(e, c.binding)) continue;
        if (typing && !c.binding.ctrlKey && !c.binding.metaKey && !c.binding.altKey) {
          continue;
        }
        e.preventDefault();
        e.stopPropagation();
        contributions.invokeCommand(c.commandId);
        return;
      }
    }

    document.addEventListener("keydown", handler, true);
    return () => document.removeEventListener("keydown", handler, true);
  }, [compiled]);

  return null;
}
