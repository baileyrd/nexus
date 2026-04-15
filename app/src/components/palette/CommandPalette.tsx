import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  contributions,
  usePaletteCommands,
  type PaletteCommand,
} from "../../contributions";
import { isCapturing } from "../../keybindings/capture-state";
import { usePaletteStore } from "../../stores/palette";
import { Icon } from "../Icon";
import { fuzzyRank } from "./fuzzy";

/**
 * ⌘K / Ctrl+K command palette. Iterates registered palette commands,
 * fuzzy-ranks against the query, dispatches selections through the
 * contribution registry.
 *
 * Mounted once at the app root; renders nothing until
 * `usePaletteStore.open` is true. Binds the global Cmd/Ctrl+K handler
 * at mount.
 */
export function CommandPalette() {
  const open = usePaletteStore((s) => s.open);
  const closePalette = usePaletteStore((s) => s.closePalette);
  const togglePalette = usePaletteStore((s) => s.togglePalette);

  useGlobalToggle(togglePalette);

  if (!open) return null;
  return <PaletteDialog onClose={closePalette} />;
}

function useGlobalToggle(togglePalette: () => void) {
  useEffect(() => {
    function handler(e: KeyboardEvent) {
      // Don't hijack Cmd/Ctrl+K when the Hotkeys tab is recording a
      // new chord — the user might be trying to rebind the palette.
      if (isCapturing()) return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        togglePalette();
      }
    }
    // Capture phase so we intercept Cmd/Ctrl+K before any focused
    // input consumes the keystroke.
    document.addEventListener("keydown", handler, true);
    return () => document.removeEventListener("keydown", handler, true);
  }, [togglePalette]);
}

interface PaletteDialogProps {
  onClose: () => void;
}

function PaletteDialog({ onClose }: PaletteDialogProps) {
  const items = usePaletteCommands();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  const ranked = useMemo(
    () => fuzzyRank(items, query.trim(), (c) => `${c.category ?? ""} ${c.title}`),
    [items, query],
  );

  // Keep the selected index in bounds when the filtered list changes.
  useEffect(() => {
    setSelectedIndex((i) => {
      if (ranked.length === 0) return 0;
      return Math.min(i, ranked.length - 1);
    });
  }, [ranked]);

  // Reset to top when query changes (not when list re-renders for the
  // same query).
  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  function commit(item: PaletteCommand) {
    onClose();
    // Defer dispatch until after the modal unmounts so stack traces are
    // rooted at the palette action, not inside the unmount commit.
    queueMicrotask(() => contributions.invokeCommand(item.commandId));
  }

  function handleKeyDown(e: ReactKeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (ranked.length === 0) return;
      setSelectedIndex((i) => (i + 1) % ranked.length);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (ranked.length === 0) return;
      setSelectedIndex((i) => (i - 1 + ranked.length) % ranked.length);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const choice = ranked[selectedIndex];
      if (choice) commit(choice.item);
    }
  }

  // Scroll the selected row into view on change.
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const row = list.querySelector<HTMLLIElement>(`[data-index="${selectedIndex}"]`);
    row?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  return (
    <div
      className="palette-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="palette-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onKeyDown={handleKeyDown}
      >
        <input
          className="palette-input"
          type="text"
          autoFocus
          placeholder="Type a command…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-controls="palette-results"
          aria-activedescendant={
            ranked.length > 0 ? `palette-row-${selectedIndex}` : undefined
          }
        />
        <ul
          ref={listRef}
          id="palette-results"
          className="palette-results"
          role="listbox"
        >
          {ranked.length === 0 ? (
            <li className="palette-empty">no commands match</li>
          ) : (
            ranked.map(({ item }, index) => (
              <li
                key={item.id}
                id={`palette-row-${index}`}
                data-index={index}
                role="option"
                aria-selected={index === selectedIndex}
                className={
                  index === selectedIndex
                    ? "palette-row selected"
                    : "palette-row"
                }
                onMouseEnter={() => setSelectedIndex(index)}
                onClick={() => commit(item)}
              >
                {item.icon && (
                  <Icon name={item.icon} size={16} className="palette-row-icon" />
                )}
                <span className="palette-row-title">{item.title}</span>
                {item.category && (
                  <span className="palette-row-category">{item.category}</span>
                )}
                {item.keybinding && (
                  <span className="palette-row-keybinding">{item.keybinding}</span>
                )}
              </li>
            ))
          )}
        </ul>
      </div>
    </div>
  );
}
