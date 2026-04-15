import type { PanelToolbarItem } from "../../bindings";
import { contributions } from "../../contributions";
import { Icon } from "../Icon";

interface PanelToolbarProps {
  items: PanelToolbarItem[];
  onTogglePanel: (panelId: string) => void;
}

/**
 * Horizontal row of icon buttons rendered at the top of an active
 * sidebar panel (part 2 of the three-layer sidebar: ribbon / toolbar /
 * content). Items are contributed by the plugin that owns the panel.
 */
export function PanelToolbar({ items, onTogglePanel }: PanelToolbarProps) {
  if (items.length === 0) return null;
  return (
    <div className="panel-toolbar" role="toolbar" aria-label="Panel actions">
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          className="panel-toolbar-item"
          title={item.tooltip}
          aria-label={item.tooltip}
          onClick={() => handleToolbarClick(item, onTogglePanel)}
        >
          <Icon name={item.icon} size={16} />
        </button>
      ))}
    </div>
  );
}

function handleToolbarClick(
  item: PanelToolbarItem,
  onTogglePanel: (panelId: string) => void,
) {
  switch (item.action.kind) {
    case "togglePanel":
      onTogglePanel(item.action.panelId);
      return;
    case "invokeCommand":
      contributions.invokeCommand(item.action.command);
      return;
    case "openView":
      contributions.openView(item.action.viewId);
      return;
  }
}
