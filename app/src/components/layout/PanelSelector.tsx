import type { Panel } from "../../bindings";
import { Icon } from "../Icon";

interface PanelSelectorProps {
  panels: Panel[];
  /** Called with the clicked panel's id. Caller decides whether to
   *  toggle visibility, activate exclusively, etc. */
  onSelect: (panelId: string) => void;
  /** Optional aria-label override (chrome row uses "Left panels" /
   *  "Right panels" rather than a generic "Panel selector"). */
  label?: string;
}

/**
 * Horizontal row of panel-selector buttons. Rendered in the workspace
 * chrome — one cluster for the left side panel's panels, one for the
 * right. Each button is derived from a [`Panel`] — icon + title come
 * straight off the panel.
 *
 * Toolbar 1 in the user's three-layer side-panel model.
 */
export function PanelSelector({ panels, onSelect, label }: PanelSelectorProps) {
  if (panels.length === 0) return null;
  return (
    <div
      className="panel-selector"
      role="tablist"
      aria-label={label ?? "Panel selector"}
    >
      {panels.map((p) => (
        <button
          key={p.id}
          type="button"
          role="tab"
          aria-selected={p.visible}
          className={p.visible ? "panel-selector-item active" : "panel-selector-item"}
          title={p.title}
          aria-label={p.title}
          onClick={() => onSelect(p.id)}
        >
          <Icon name={p.icon} size={16} />
        </button>
      ))}
    </div>
  );
}
