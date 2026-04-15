import type { RibbonItem } from "../../bindings";
import { contributions } from "../../contributions";
import { Icon } from "../Icon";

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
 * resolved through the UI contribution registry (pending §8).
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
          <Icon name={item.icon} size={18} className="ribbon-icon" />
        </button>
      ))}
    </>
  );
}

function handleRibbonClick(item: RibbonItem) {
  switch (item.action.kind) {
    case "togglePanel":
      // Ribbon-level togglePanel doesn't name a side (the data model
      // assumes a single side panel at the workspace level); will be
      // resolved once ribbon items carry a target side.
      // eslint-disable-next-line no-console
      console.log(
        `[ribbon] togglePanel '${item.action.panelId}' from workspace ribbon (target side panel pending)`,
      );
      return;
    case "invokeCommand":
      contributions.invokeCommand(item.action.command);
      return;
    case "openView":
      contributions.openView(item.action.viewId);
      return;
  }
}
