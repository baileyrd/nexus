import { useEffect, useRef } from "react";

export interface ContextMenuItem {
  /** Stable id (not rendered). */
  id: string;
  /** User-visible label. */
  label: string;
  /** Action to run when picked. */
  onSelect: () => void | Promise<void>;
  /** If true, render a separator above this item. */
  separatorBefore?: boolean;
  /** If true, dim the item. The action is still invoked unless caller
   *  swaps in a no-op. */
  disabled?: boolean;
}

interface ContextMenuProps {
  /** Viewport coordinates. Caller should clamp to keep the menu on-screen. */
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
  /** `"below"` (default) anchors the menu's top-left at (x, y).
   *  `"above"` anchors the menu's bottom-left at (x, y), useful for
   *  buttons near the viewport bottom (e.g. side-panel footer). */
  placement?: "above" | "below";
}

/**
 * Lightweight floating context menu. Closes on outside click, Escape,
 * blur, or after picking an item. No portal — relies on `position: fixed`
 * with high z-index. Sufficient until we need nested menus.
 */
export function ContextMenu({
  x,
  y,
  items,
  onClose,
  placement = "below",
}: ContextMenuProps) {
  const ref = useRef<HTMLUListElement>(null);

  useEffect(() => {
    function onPointerDown(e: PointerEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  const style: React.CSSProperties = {
    position: "fixed",
    top: y,
    left: x,
    transform: placement === "above" ? "translateY(-100%)" : undefined,
  };

  return (
    <ul ref={ref} className="context-menu" role="menu" style={style}>
      {items.map((item) => (
        <li
          key={item.id}
          className={
            item.separatorBefore
              ? "context-menu-item with-separator"
              : "context-menu-item"
          }
        >
          <button
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onClick={() => {
              void item.onSelect();
              onClose();
            }}
          >
            {item.label}
          </button>
        </li>
      ))}
    </ul>
  );
}
