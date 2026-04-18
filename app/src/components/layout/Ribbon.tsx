import type { RibbonItem } from "../../bindings";
import { contributions } from "../../contributions";
import { useLayoutStore } from "../../stores/layout";
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
    case "togglePanel": {
      const { panelId } = item.action;
      const {
        layout,
        activatePanel,
        togglePanelVisibility,
        toggleSidePanelCollapsed,
      } = useLayoutStore.getState();
      if (!layout) return;
      const inLeft = layout.leftSidePanel.panels.some((p) => p.id === panelId);
      const side = inLeft ? "left" : "right";
      const sidePanel = inLeft ? layout.leftSidePanel : layout.rightSidePanel;
      const activePanel = sidePanel.panels.find((p) => p.visible);
      // Rail single-select semantics: clicking the already-active panel's
      // rail icon collapses the side panel; clicking any other makes it
      // the sole visible panel and uncollapses. Matches VS Code's
      // activity-bar behavior.
      if (activePanel?.id === panelId && !sidePanel.collapsed) {
        toggleSidePanelCollapsed(side);
        return;
      }
      if (sidePanel.collapsed) toggleSidePanelCollapsed(side);
      if (activePanel?.id !== panelId) {
        activatePanel(side, panelId);
        // `activatePanel` only swaps the active panel among already-visible
        // ones; if the target was hidden (common when togglePanel is used
        // from the rail), also flip its visibility on.
        if (!sidePanel.panels.find((p) => p.id === panelId)?.visible) {
          togglePanelVisibility(side, panelId);
        }
      }
      return;
    }
    case "invokeCommand":
      contributions.invokeCommand(item.action.command);
      return;
    case "openView":
      contributions.openView(item.action.viewId);
      return;
  }
}
