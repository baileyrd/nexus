import type { RibbonItem } from "../../bindings";

interface RibbonProps {
  side: "left" | "right";
  items: RibbonItem[];
  activePanelIds: Set<string>;
  onTogglePanel: (panelId: string) => void;
}

/**
 * Narrow icon rail rendered at the docked edge of a sidebar.
 *
 * Until the icon registry lands (§13) and a UI contribution registry
 * (§8) resolves `command` / `viewId` ids, icons render as the first
 * letter of the ribbon item id and the only wired action is
 * `togglePanel`. Other actions log to the console so plugins can
 * register stubs and see them fire.
 */
export function Ribbon({ side, items, activePanelIds, onTogglePanel }: RibbonProps) {
  return (
    <nav className="ribbon" data-side={side} aria-label={`${side} ribbon`}>
      {items.map((item) => {
        const isActive =
          item.action.kind === "togglePanel" && activePanelIds.has(item.action.panelId);
        return (
          <button
            key={item.id}
            type="button"
            className={isActive ? "ribbon-item active" : "ribbon-item"}
            title={item.tooltip}
            aria-label={item.tooltip}
            aria-pressed={isActive}
            onClick={() => handleRibbonClick(item, onTogglePanel)}
          >
            <span aria-hidden="true" className="ribbon-icon">
              {placeholderGlyph(item.icon)}
            </span>
          </button>
        );
      })}
    </nav>
  );
}

function handleRibbonClick(
  item: RibbonItem,
  onTogglePanel: (panelId: string) => void,
) {
  switch (item.action.kind) {
    case "togglePanel":
      onTogglePanel(item.action.panelId);
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
