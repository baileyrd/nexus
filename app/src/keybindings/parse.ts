// Keybinding parser + matcher.
//
// Format: `+`-separated chord of modifiers and a key, case-insensitive.
// Modifiers: Mod, Cmd, Ctrl, Meta, Alt, Shift.
//   - `Mod` normalises to Cmd on macOS, Ctrl everywhere else — so a
//     cross-platform "native feel" binding can be declared once.
//   - `Cmd` = metaKey; `Ctrl` = ctrlKey; `Meta` is treated as metaKey too.
// Key: the final segment is the literal key, e.g. `K`, `/`, `Escape`,
// `F1`. Matched against `KeyboardEvent.key.toLowerCase()`.

/**
 * Normalised keybinding — a predicate over a keyboard event.
 * Produced by `parseKeybinding`; consumed by `matchesBinding`.
 */
export interface ParsedBinding {
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  /** Final key, lowercase (`"k"`, `"escape"`, `"/"`). */
  key: string;
}

const isMac = (): boolean => {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPod|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
};

/**
 * Parse a `+`-separated keybinding string into a normalised form.
 * Returns `null` if the string is malformed (empty, no key segment,
 * unknown modifier) — callers should treat that as "no binding".
 */
export function parseKeybinding(raw: string): ParsedBinding | null {
  if (!raw) return null;
  const segments = raw.split("+").map((s) => s.trim()).filter(Boolean);
  if (segments.length === 0) return null;

  const key = segments[segments.length - 1].toLowerCase();
  const mods = segments.slice(0, -1).map((s) => s.toLowerCase());

  const out: ParsedBinding = {
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    key,
  };

  const mac = isMac();
  for (const mod of mods) {
    switch (mod) {
      case "mod":
        if (mac) out.metaKey = true;
        else out.ctrlKey = true;
        break;
      case "cmd":
      case "meta":
      case "super":
        out.metaKey = true;
        break;
      case "ctrl":
      case "control":
        out.ctrlKey = true;
        break;
      case "alt":
      case "option":
        out.altKey = true;
        break;
      case "shift":
        out.shiftKey = true;
        break;
      default:
        return null;
    }
  }
  return out;
}

/** Does `event` match `binding`? All modifier flags must match exactly. */
export function matchesBinding(event: KeyboardEvent, binding: ParsedBinding): boolean {
  return (
    event.ctrlKey === binding.ctrlKey &&
    event.metaKey === binding.metaKey &&
    event.altKey === binding.altKey &&
    event.shiftKey === binding.shiftKey &&
    event.key.toLowerCase() === binding.key
  );
}
