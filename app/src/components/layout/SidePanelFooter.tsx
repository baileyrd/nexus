import type { FooterAction, SidePanelFooter as SidePanelFooterData } from "../../bindings";

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
          onClick={() => {
            // eslint-disable-next-line no-console
            console.log("[footer] switch forge (registry pending)");
          }}
        >
          <span aria-hidden="true" className="forge-selector-chevron">
            ⇅
          </span>
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
            <span aria-hidden="true">{placeholderGlyph(action.icon)}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

function handleFooterClick(action: FooterAction) {
  switch (action.action.kind) {
    case "togglePanel":
      // eslint-disable-next-line no-console
      console.log(`[footer] togglePanel '${action.action.panelId}' (target side panel pending)`);
      return;
    case "invokeCommand":
      // eslint-disable-next-line no-console
      console.log(`[footer] invoke command '${action.action.command}' (registry pending)`);
      return;
    case "openView":
      // eslint-disable-next-line no-console
      console.log(`[footer] open view '${action.action.viewId}' (registry pending)`);
      return;
  }
}

function placeholderGlyph(icon: string): string {
  const trimmed = icon.trim();
  if (!trimmed) return "◇";
  return trimmed[0].toUpperCase();
}
