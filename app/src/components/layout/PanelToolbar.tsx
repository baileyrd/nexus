import type { PanelToolbarItem } from "../../bindings";

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
          <span aria-hidden="true">{placeholderGlyph(item.icon)}</span>
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
      // eslint-disable-next-line no-console
      console.log(`[panel-toolbar] invoke command '${item.action.command}' (registry pending)`);
      return;
    case "openView":
      // eslint-disable-next-line no-console
      console.log(`[panel-toolbar] open view '${item.action.viewId}' (registry pending)`);
      return;
  }
}

function placeholderGlyph(icon: string): string {
  const trimmed = icon.trim();
  if (!trimmed) return "◇";
  return trimmed[0].toUpperCase();
}
