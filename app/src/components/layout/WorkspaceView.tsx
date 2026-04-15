import { useEffect } from "react";
import type { Panel, SidePanel } from "../../bindings";
import { useLayoutStore } from "../../stores/layout";
import { LayoutPresetPicker } from "./LayoutPresetPicker";
import { PanelSelector } from "./PanelSelector";
import { PanelToolbar } from "./PanelToolbar";
import { Ribbon } from "./Ribbon";
import { SidePanelFooter } from "./SidePanelFooter";
import { SplitPane } from "./SplitPane";

export function WorkspaceView() {
  const layout = useLayoutStore((s) => s.layout);
  const load = useLayoutStore((s) => s.load);
  const loading = useLayoutStore((s) => s.loading);
  const error = useLayoutStore((s) => s.error);
  const togglePanelVisibility = useLayoutStore((s) => s.togglePanelVisibility);
  const toggleSidePanelCollapsed = useLayoutStore((s) => s.toggleSidePanelCollapsed);

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
        <div className="workspace-frame" data-workspace-name={layout.name}>
          <div className="workspace-chrome">
            <SidePanelToggle
              side="left"
              collapsed={layout.leftSidePanel.collapsed}
              onClick={() => toggleSidePanelCollapsed("left")}
            />
            <SidePanelToggle
              side="right"
              collapsed={layout.rightSidePanel.collapsed}
              onClick={() => toggleSidePanelCollapsed("right")}
            />
          </div>
          <div className="workspace-body">
            {layout.ribbon.length > 0 && <Ribbon items={layout.ribbon} />}
            {!layout.leftSidePanel.collapsed && (
              <SidePanelView
                side="left"
                sidePanel={layout.leftSidePanel}
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
            {!layout.rightSidePanel.collapsed && (
              <SidePanelView
                side="right"
                sidePanel={layout.rightSidePanel}
                onTogglePanel={(id) => togglePanelVisibility("right", id)}
              />
            )}
          </div>
        </div>
      ) : loading ? (
        <p className="hint">loading layout…</p>
      ) : null}
    </section>
  );
}

interface SidePanelToggleProps {
  side: "left" | "right";
  collapsed: boolean;
  onClick: () => void;
}

function SidePanelToggle({ side, collapsed, onClick }: SidePanelToggleProps) {
  const label = `${collapsed ? "Show" : "Hide"} ${side} side panel`;
  // Glyph mirrors Obsidian's: a small box with a vertical bar on the
  // matching edge. Filled when expanded, hollow when collapsed.
  const glyph = side === "left"
    ? collapsed ? "▢" : "◧"
    : collapsed ? "▢" : "◨";
  return (
    <button
      type="button"
      className="side-panel-toggle"
      data-side={side}
      data-collapsed={collapsed}
      title={label}
      aria-label={label}
      aria-pressed={!collapsed}
      onClick={onClick}
    >
      <span aria-hidden="true">{glyph}</span>
    </button>
  );
}

interface SidePanelViewProps {
  side: "left" | "right";
  sidePanel: SidePanel;
  onTogglePanel: (panelId: string) => void;
}

/**
 * One docked side panel. Three stacked surfaces:
 *
 *   1. Panel-selector toolbar (horizontal, derived from `panels`)
 *   2. Active panel's local toolbar (in the panel header)
 *   3. Active panel's content area
 *
 * The workspace activity ribbon is rendered separately by
 * `WorkspaceView` — it isn't part of the side panel.
 */
function SidePanelView({ side, sidePanel, onTogglePanel }: SidePanelViewProps) {
  const activePanel = sidePanel.panels.find((p) => p.visible) ?? null;

  return (
    <aside className="side-panel-preview" data-side={side}>
      <PanelSelector panels={sidePanel.panels} onTogglePanel={onTogglePanel} />
      <div className="panel-area">
        {activePanel ? (
          <PanelView panel={activePanel} onTogglePanel={onTogglePanel} />
        ) : (
          <div className="panel-empty">no panel selected</div>
        )}
      </div>
      {sidePanel.footer && (
        <SidePanelFooter footer={sidePanel.footer} forgeName="lap-working" />
      )}
    </aside>
  );
}

interface PanelViewProps {
  panel: Panel;
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
