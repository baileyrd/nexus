import { useEffect } from "react";
import type { RibbonItem, SidebarPanel } from "../../bindings";
import { useLayoutStore } from "../../stores/layout";
import { LayoutPresetPicker } from "./LayoutPresetPicker";
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
              ribbon={layout.rightSidebar.ribbon}
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

function SidebarPreview({ side, panels, ribbon, onTogglePanel }: SidebarPreviewProps) {
  const activePanelIds = new Set(panels.filter((p) => p.visible).map((p) => p.id));

  const panelList = (
    <ul className="sidebar-panels">
      {panels.map((p) => (
        <li key={p.id} className={p.visible ? "active" : undefined}>
          <span className="icon" aria-hidden>
            ◇
          </span>
          <span className="title">{p.title}</span>
        </li>
      ))}
      {panels.length === 0 && <li className="empty">no panels</li>}
    </ul>
  );

  const ribbonRail = ribbon.length > 0 ? (
    <Ribbon
      side={side}
      items={ribbon}
      activePanelIds={activePanelIds}
      onTogglePanel={onTogglePanel}
    />
  ) : null;

  // Obsidian layout: ribbon sits on the docked edge. For the left
  // sidebar that means ribbon first, then panels; for the right
  // sidebar it's panels then ribbon.
  return (
    <aside className="sidebar-preview" data-side={side}>
      {side === "left" ? (
        <>
          {ribbonRail}
          {panelList}
        </>
      ) : (
        <>
          {panelList}
          {ribbonRail}
        </>
      )}
    </aside>
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
