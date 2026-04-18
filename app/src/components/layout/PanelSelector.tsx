import type { Panel } from "../../bindings";
import { usePanelCount } from "../../stores/panelCounts";
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
        <PanelSelectorItem key={p.id} panel={p} onSelect={onSelect} />
      ))}
    </div>
  );
}

interface PanelSelectorItemProps {
  panel: Panel;
  onSelect: (panelId: string) => void;
}

function PanelSelectorItem({ panel, onSelect }: PanelSelectorItemProps) {
  const count = usePanelCount(panel.id);
  return (
    <button
      type="button"
      role="tab"
      aria-selected={panel.visible}
      className={
        panel.visible ? "panel-selector-item active" : "panel-selector-item"
      }
      title={panel.title}
      aria-label={
        count !== undefined ? `${panel.title} (${count})` : panel.title
      }
      onClick={() => onSelect(panel.id)}
    >
      <Icon name={panel.icon} size={16} />
      <span className="panel-selector-label">{panel.title}</span>
      {count !== undefined && (
        <span className="panel-selector-count" aria-hidden="true">
          {count}
        </span>
      )}
    </button>
  );
}
