import type { RibbonItem } from "../../bindings";

interface RibbonItemsProps {
  items: RibbonItem[];
}

/**
 * The button list inside the workspace ribbon. Rendered without a
 * surrounding `<nav>` so the parent (`WorkspaceView`) can place it next
 * to the left side-panel toggle in the same `<nav className="ribbon">`
 * column.
 *
 * Items are plugin/view shortcuts (graph view, calendar, terminal, …)
 * resolved through the UI contribution registry (pending §8 / §13).
 */
export function RibbonItems({ items }: RibbonItemsProps) {
  if (items.length === 0) return null;
  return (
    <>
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
    </>
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
