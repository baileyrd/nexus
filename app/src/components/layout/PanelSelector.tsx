import type { SidebarPanel } from "../../bindings";

interface PanelSelectorProps {
  panels: SidebarPanel[];
  onTogglePanel: (panelId: string) => void;
}

/**
 * Horizontal row of panel-selector buttons rendered at the top of a
 * sidebar that doesn't use a vertical ribbon (the right sidebar in the
 * Obsidian reference). Each button is derived from a [`SidebarPanel`]
 * — icon + title come straight off the panel, click toggles visibility.
 *
 * Toolbar 1 in the user's model.
 */
export function PanelSelector({ panels, onTogglePanel }: PanelSelectorProps) {
  if (panels.length === 0) return null;
  return (
    <div className="panel-selector" role="tablist" aria-label="Panel selector">
      {panels.map((p) => (
        <button
          key={p.id}
          type="button"
          role="tab"
          aria-selected={p.visible}
          className={p.visible ? "panel-selector-item active" : "panel-selector-item"}
          title={p.title}
          aria-label={p.title}
          onClick={() => onTogglePanel(p.id)}
        >
          <span aria-hidden="true">{placeholderGlyph(p.icon)}</span>
        </button>
      ))}
    </div>
  );
}

function placeholderGlyph(icon: string): string {
  const trimmed = icon.trim();
  if (!trimmed) return "◇";
  return trimmed[0].toUpperCase();
}
