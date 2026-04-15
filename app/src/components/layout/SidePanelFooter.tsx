import { useRef, useState } from "react";
import type {
  FooterAction,
  SidePanelFooter as SidePanelFooterData,
} from "../../bindings";
import { contributions } from "../../contributions";
import { useForgeStore } from "../../stores/forge";
import { useLayoutStore } from "../../stores/layout";
import { ContextMenu, type ContextMenuItem } from "../ContextMenu";
import { Icon } from "../Icon";

interface SidePanelFooterProps {
  footer: SidePanelFooterData;
  /** Forge name to display in the selector. */
  forgeName?: string;
}

/**
 * Footer row pinned at the bottom of a side panel. Forge selector on
 * the left (when enabled), action icons on the right. Clicking the
 * selector opens a menu of recent forges plus an "Open folder…" entry
 * that falls through to the directory picker.
 */
export function SidePanelFooter({
  footer,
  forgeName = "—",
}: SidePanelFooterProps) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const recent = useLayoutStore((s) => s.persistence?.recentForgePaths ?? []);
  const currentRoot = useForgeStore((s) => s.info?.root);
  const openForge = useForgeStore((s) => s.open);

  const visibleRecent = recent.filter((p) => p !== currentRoot);
  const hasRecent = visibleRecent.length > 0;

  const items: ContextMenuItem[] = [
    ...visibleRecent.map((path, i) => ({
      id: `recent-${i}`,
      label: path,
      onSelect: () => void openForge(path),
    })),
    {
      id: "open-folder",
      label: "Open folder…",
      separatorBefore: hasRecent,
      onSelect: () => contributions.invokeCommand("workspace.switch-forge"),
    },
  ];

  const onSelectorClick = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (!rect) return;
    setMenu({ x: rect.left, y: rect.top });
  };

  return (
    <div className="side-panel-footer">
      {footer.showForgeSelector && (
        <button
          ref={buttonRef}
          type="button"
          className="forge-selector"
          title="Switch forge"
          onClick={onSelectorClick}
        >
          <Icon
            name="chevrons-up-down"
            size={14}
            className="forge-selector-chevron"
          />
          <span className="forge-selector-name">{forgeName}</span>
        </button>
      )}
      <div className="side-panel-footer-actions">
        {footer.actions.map((action) => (
          <button
            key={action.id}
            type="button"
            className="side-panel-footer-action"
            title={action.tooltip}
            aria-label={action.tooltip}
            onClick={() => handleFooterClick(action)}
          >
            <Icon name={action.icon} size={16} />
          </button>
        ))}
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={items}
          placement="above"
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

function handleFooterClick(action: FooterAction) {
  switch (action.action.kind) {
    case "togglePanel":
      // Footer-level togglePanel needs a side resolution (same as ribbon);
      // left as a log until ribbon/footer actions carry a target side.
      // eslint-disable-next-line no-console
      console.log(
        `[footer] togglePanel '${action.action.panelId}' (target side panel pending)`,
      );
      return;
    case "invokeCommand":
      contributions.invokeCommand(action.action.command);
      return;
    case "openView":
      contributions.openView(action.action.viewId);
      return;
  }
}
