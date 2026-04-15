import type { FooterAction, SidePanelFooter as SidePanelFooterData } from "../../bindings";
import { contributions } from "../../contributions";
import { Icon } from "../Icon";

interface SidePanelFooterProps {
  footer: SidePanelFooterData;
  /** Forge name to display in the selector. Stub for now — the real value
   * will come from the active forge once that's wired through IPC. */
  forgeName?: string;
}

/**
 * Footer row pinned at the bottom of a side panel. Forge selector on
 * the left (when enabled), action icons on the right.
 */
export function SidePanelFooter({ footer, forgeName = "—" }: SidePanelFooterProps) {
  return (
    <div className="side-panel-footer">
      {footer.showForgeSelector && (
        <button
          type="button"
          className="forge-selector"
          title="Switch forge"
          onClick={() => contributions.invokeCommand("workspace.switch-forge")}
        >
          <Icon name="chevrons-up-down" size={14} className="forge-selector-chevron" />
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
    </div>
  );
}

function handleFooterClick(action: FooterAction) {
  switch (action.action.kind) {
    case "togglePanel":
      // Footer-level togglePanel needs a side resolution (same as ribbon);
      // left as a log until ribbon/footer actions carry a target side.
      // eslint-disable-next-line no-console
      console.log(`[footer] togglePanel '${action.action.panelId}' (target side panel pending)`);
      return;
    case "invokeCommand":
      contributions.invokeCommand(action.action.command);
      return;
    case "openView":
      contributions.openView(action.action.viewId);
      return;
  }
}
