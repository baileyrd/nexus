import { useCallback, useEffect, useRef, useState } from "react";
import { beginCapture, endCapture } from "./capture-state";

interface KeyCaptureInputProps {
  /** Current effective binding (may be the manifest default). */
  value: string | undefined;
  /** `true` if `value` is a user override — enables the reset affordance. */
  hasOverride: boolean;
  /** Called when the user commits a new chord. */
  onCommit: (binding: string) => void;
  /** Called when the user clicks "reset" on an existing override. */
  onReset: () => void;
}

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "OS"]);

function buildChord(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.metaKey) parts.push("Cmd");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  // KeyboardEvent.key is already human-readable ("a", "/", "Enter", "F1").
  // Normalise single letters to uppercase for consistency with how
  // bindings are displayed elsewhere (e.g. "Ctrl+K" vs "Ctrl+k").
  const key = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  parts.push(key);
  return parts.join("+");
}

/**
 * Chip-style control that displays a keybinding and, on click, enters
 * a capture mode where the next pressed chord is committed. Esc
 * cancels without saving. A small "reset" button appears next to the
 * chip when a user override is in effect.
 */
export function KeyCaptureInput({
  value,
  hasOverride,
  onCommit,
  onReset,
}: KeyCaptureInputProps) {
  const [capturing, setCapturing] = useState(false);
  const chipRef = useRef<HTMLButtonElement>(null);

  const stopCapture = useCallback(() => {
    setCapturing(false);
    // Return focus to the chip so repeated edits don't require re-tab.
    chipRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!capturing) return undefined;
    beginCapture();

    function handler(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        stopCapture();
        return;
      }
      const chord = buildChord(e);
      if (chord === null) return; // bare modifier press; keep waiting
      e.preventDefault();
      e.stopPropagation();
      onCommit(chord);
      stopCapture();
    }

    document.addEventListener("keydown", handler, true);
    return () => {
      document.removeEventListener("keydown", handler, true);
      endCapture();
    };
  }, [capturing, onCommit, stopCapture]);

  if (capturing) {
    return (
      <span className="keybinding-capture" role="status">
        Press a chord… <kbd>Esc</kbd> to cancel
      </span>
    );
  }

  return (
    <span className="keybinding-chip-group">
      <button
        ref={chipRef}
        type="button"
        className={
          value
            ? hasOverride
              ? "keybinding-chip overridden"
              : "keybinding-chip"
            : "keybinding-chip empty"
        }
        onClick={() => setCapturing(true)}
        aria-label={
          value ? `Change keybinding (currently ${value})` : "Set keybinding"
        }
      >
        {value ?? "unassigned"}
      </button>
      {hasOverride && (
        <button
          type="button"
          className="keybinding-reset"
          onClick={onReset}
          aria-label="Reset to default"
          title="Reset to default"
        >
          ×
        </button>
      )}
    </span>
  );
}
