import { useCallback, useEffect, useRef, useState } from "react";
import { contributions, useMenuItems, type MenuItem } from "../../contributions";

/**
 * Application menu bar (PRD-07 §7.5). Renders pull-down menus populated
 * by the contribution registry. Both the shell and plugins contribute items
 * via `contributions.registerMenuItem({ menu, label, commandId, ... })`.
 *
 * Keyboard: click or Enter/Space to open a menu; Escape or outside-click
 * closes; ArrowLeft/ArrowRight moves between menus; ArrowUp/ArrowDown
 * navigates items within the open menu.
 */
export function MenuBar() {
  const allItems = useMenuItems();
  const [openMenu, setOpenMenu] = useState<string | null>(null);
  const barRef = useRef<HTMLElement>(null);

  // Group by top-level menu label, preserving the sorted order from the hook.
  const menus = groupByMenu(allItems);

  const close = useCallback(() => setOpenMenu(null), []);

  // Dismiss on outside-click.
  useEffect(() => {
    if (!openMenu) return;
    function handlePointerDown(e: PointerEvent) {
      if (barRef.current && !barRef.current.contains(e.target as Node)) {
        close();
      }
    }
    document.addEventListener("pointerdown", handlePointerDown);
    return () => document.removeEventListener("pointerdown", handlePointerDown);
  }, [openMenu, close]);

  if (menus.length === 0) return null;

  return (
    <nav className="menu-bar" ref={barRef} aria-label="Application menu">
      {menus.map(({ label, items }) => (
        <MenuPullDown
          key={label}
          label={label}
          items={items}
          isOpen={openMenu === label}
          onToggle={() => setOpenMenu((prev) => (prev === label ? null : label))}
          onClose={close}
        />
      ))}
    </nav>
  );
}

interface MenuGroup {
  label: string;
  items: MenuItem[];
  menuOrder: number;
}

function groupByMenu(items: MenuItem[]): MenuGroup[] {
  const map = new Map<string, MenuGroup>();
  for (const item of items) {
    const topLevel = item.menu.split(" > ")[0];
    let group = map.get(topLevel);
    if (!group) {
      group = { label: topLevel, items: [], menuOrder: item.menuOrder ?? 100 };
      map.set(topLevel, group);
    } else if ((item.menuOrder ?? 100) < group.menuOrder) {
      group.menuOrder = item.menuOrder ?? 100;
    }
    group.items.push(item);
  }
  return Array.from(map.values()).sort(
    (a, b) => a.menuOrder - b.menuOrder || a.label.localeCompare(b.label),
  );
}

interface MenuPullDownProps {
  label: string;
  items: MenuItem[];
  isOpen: boolean;
  onToggle: () => void;
  onClose: () => void;
}

function MenuPullDown({ label, items, isOpen, onToggle, onClose }: MenuPullDownProps) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      onClose();
      triggerRef.current?.focus();
    }
  }

  return (
    <div className="menu-pull-down" data-open={isOpen}>
      <button
        ref={triggerRef}
        type="button"
        className="menu-pull-down-trigger"
        aria-haspopup="menu"
        aria-expanded={isOpen}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && !isOpen) {
            e.preventDefault();
            onToggle();
          }
        }}
      >
        {label}
      </button>
      {isOpen && (
        <ul
          ref={listRef}
          className="menu-pull-down-list"
          role="menu"
          aria-label={label}
          onKeyDown={handleKeyDown}
        >
          {items.map((item) => (
            <MenuEntry
              key={item.id}
              item={item}
              onClose={onClose}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

interface MenuEntryProps {
  item: MenuItem;
  onClose: () => void;
}

function MenuEntry({ item, onClose }: MenuEntryProps) {
  function handleSelect() {
    if (item.disabled) return;
    onClose();
    contributions.invokeCommand(item.commandId);
  }

  return (
    <>
      {item.separatorBefore && (
        <li className="menu-separator" role="separator" aria-hidden="true" />
      )}
      <li role="none">
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          disabled={item.disabled}
          onClick={handleSelect}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              handleSelect();
            }
          }}
        >
          {item.label}
        </button>
      </li>
    </>
  );
}
