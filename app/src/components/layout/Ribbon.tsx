import type { RibbonItem } from "../../bindings";

interface RibbonProps {
  items: RibbonItem[];
}

/**
 * Workspace activity ribbon — the narrow vertical icon rail docked to
 * the far-left edge of the window, independent of either side panel.
 *
 * Items are plugin/view shortcuts (graph view, calendar, terminal, …)
 * resolved through the UI contribution registry (pending §8 / §13).
 * Until that lands, icons render as the first letter of the ribbon
 * item id and `invokeCommand` / `openView` clicks log to the console.
 */
export function Ribbon({ items }: RibbonProps) {
  if (items.length === 0) return null;
  return (
    <nav className="ribbon" aria-label="Workspace ribbon">
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          className="ribbon-item"
          title={item.tooltip}
          aria-label={item.tooltip}
          onClick={() => handleRibbonClick(item)}
        >
          <span aria-hidden="true" className="ribbon-icon">
            {placeholderGlyph(item.icon)}
          </span>
        </button>
      ))}
    </nav>
  );
}

function handleRibbonClick(item: RibbonItem) {
  switch (item.action.kind) {
    case "togglePanel":
      // eslint-disable-next-line no-console
      console.log(
        `[ribbon] togglePanel '${item.action.panelId}' from workspace ribbon (target side panel pending)`,
      );
      return;
    case "invokeCommand":
      // eslint-disable-next-line no-console
      console.log(`[ribbon] invoke command '${item.action.command}' (registry pending)`);
      return;
    case "openView":
      // eslint-disable-next-line no-console
      console.log(`[ribbon] open view '${item.action.viewId}' (registry pending)`);
      return;
  }
}

function placeholderGlyph(icon: string): string {
  const trimmed = icon.trim();
  if (!trimmed) return "◇";
  return trimmed[0].toUpperCase();
}
