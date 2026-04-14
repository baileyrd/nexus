import { useEffect } from "react";
import type { RibbonItem, SidebarPanel } from "../../bindings";
import { useLayoutStore } from "../../stores/layout";
import { LayoutPresetPicker } from "./LayoutPresetPicker";
import { PanelToolbar } from "./PanelToolbar";
import { Ribbon } from "./Ribbon";
import { SplitPane } from "./SplitPane";

export function WorkspaceView() {
  const layout = useLayoutStore((s) => s.layout);
  const load = useLayoutStore((s) => s.load);
  const loading = useLayoutStore((s) => s.loading);
  const error = useLayoutStore((s) => s.error);
  const togglePanelVisibility = useLayoutStore((s) => s.togglePanelVisibility);

  useEffect(() => {
    if (!layout) load();
  }, [layout, load]);

  return (
    <section className="workspace-view">
      <header>
        <h2>Workspace</h2>
        <LayoutPresetPicker />
      </header>

      {error && <p className="error">Failed to load layout: {error}</p>}

      {layout ? (
        <div
          className="workspace-frame"
          data-workspace-name={layout.name}
          style={{ display: "flex" }}
        >
          {!layout.leftSidebar.collapsed && (
            <SidebarPreview
              side="left"
              panels={layout.leftSidebar.panels}
              ribbon={layout.leftSidebar.ribbon}
              onTogglePanel={(id) => togglePanelVisibility("left", id)}
            />
          )}
          <div className="workspace-center">
            <SplitPane
              node={layout.root}
              focusedPaneId={layout.focusedPaneId}
            />
            {!layout.bottomPanel.collapsed && (
              <BottomPreview height={layout.bottomPanel.height} />
            )}
          </div>
          {!layout.rightSidebar.collapsed && (
            <SidebarPreview
              side="right"
              panels={layout.rightSidebar.panels}
              ribbon={[]}
              onTogglePanel={(id) => togglePanelVisibility("right", id)}
            />
          )}
        </div>
      ) : loading ? (
        <p className="hint">loading layout…</p>
      ) : null}
    </section>
  );
}

interface SidebarPreviewProps {
  side: "left" | "right";
  panels: SidebarPanel[];
  ribbon: RibbonItem[];
  onTogglePanel: (panelId: string) => void;
}

/**
 * Three-layer sidebar (user's mental model):
 *   1. Ribbon — panel-selector icons at the docked edge (left only)
 *   2. Panel toolbar — panel-local action icons for the active panel
 *   3. Content area — the active panel's body
 */
function SidebarPreview({ side, panels, ribbon, onTogglePanel }: SidebarPreviewProps) {
  const activePanelIds = new Set(panels.filter((p) => p.visible).map((p) => p.id));
  const activePanel = panels.find((p) => p.visible) ?? null;

  return (
    <aside className="sidebar-preview" data-side={side}>
      {side === "left" && ribbon.length > 0 && (
        <Ribbon
          side={side}
          items={ribbon}
          activePanelIds={activePanelIds}
          onTogglePanel={onTogglePanel}
        />
      )}
      <div className="panel-area">
        {activePanel ? (
          <PanelView panel={activePanel} onTogglePanel={onTogglePanel} />
        ) : (
          <div className="panel-empty">no panel selected</div>
        )}
      </div>
    </aside>
  );
}

interface PanelViewProps {
  panel: SidebarPanel;
  onTogglePanel: (panelId: string) => void;
}

function PanelView({ panel, onTogglePanel }: PanelViewProps) {
  return (
    <div className="panel-view" data-panel-id={panel.id}>
      <header className="panel-header">
        <span className="panel-title">{panel.title}</span>
        <PanelToolbar items={panel.toolbar} onTogglePanel={onTogglePanel} />
      </header>
      <div className="panel-content">
        {panel.contentType ? (
          <span className="panel-content-stub">
            contentType: <code>{panel.contentType}</code>
          </span>
        ) : (
          <span className="panel-content-empty">no content renderer</span>
        )}
      </div>
    </div>
  );
}

function BottomPreview({ height }: { height: number }) {
  return (
    <div
      className="bottom-preview"
      style={{ height: `${Math.min(height, 200)}px` }}
    >
      <span>bottom panel</span>
    </div>
  );
}
