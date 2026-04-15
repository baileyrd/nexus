import { useEffect } from "react";
import type { Panel, SidePanel } from "../../bindings";
import { useLayoutStore } from "../../stores/layout";
import { LayoutPresetPicker } from "./LayoutPresetPicker";
import { PanelSelector } from "./PanelSelector";
import { PanelToolbar } from "./PanelToolbar";
import { RibbonItems } from "./Ribbon";
import { SidePanelFooter } from "./SidePanelFooter";
import { SplitPane } from "./SplitPane";
import { StatusBar } from "./StatusBar";

export function WorkspaceView() {
  const layout = useLayoutStore((s) => s.layout);
  const load = useLayoutStore((s) => s.load);
  const loading = useLayoutStore((s) => s.loading);
  const error = useLayoutStore((s) => s.error);
  const togglePanelVisibility = useLayoutStore((s) => s.togglePanelVisibility);
  const toggleSidePanelCollapsed = useLayoutStore((s) => s.toggleSidePanelCollapsed);
  const activatePanel = useLayoutStore((s) => s.activatePanel);

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
          <nav className="ribbon" aria-label="Workspace ribbon">
            <SidePanelToggle
              side="left"
              collapsed={layout.leftSidePanel.collapsed}
              onClick={() => toggleSidePanelCollapsed("left")}
            />
            <RibbonItems items={layout.ribbon} />
          </nav>
          {!layout.leftSidePanel.collapsed && (
            <SidePanelView
              side="left"
              sidePanel={layout.leftSidePanel}
              onActivate={(id) => activatePanel("left", id)}
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
          <div className="right-side-panel">
            <div className="right-side-panel-toolbar">
              <SidePanelToggle
                side="right"
                collapsed={layout.rightSidePanel.collapsed}
                onClick={() => toggleSidePanelCollapsed("right")}
              />
              {!layout.rightSidePanel.collapsed && (
                <PanelSelector
                  panels={layout.rightSidePanel.panels}
                  label="Right side panel"
                  onSelect={(id) => activatePanel("right", id)}
                />
              )}
            </div>
            {!layout.rightSidePanel.collapsed && (
              <SidePanelBody
                side="right"
                sidePanel={layout.rightSidePanel}
                onTogglePanel={(id) => togglePanelVisibility("right", id)}
              />
            )}
          </div>
          <StatusBar items={layout.statusBar} />
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
  onActivate: (panelId: string) => void;
  onTogglePanel: (panelId: string) => void;
}

/**
 * Left side panel — full height, stacked vertically:
 *
 *   1. Panel-selector toolbar (toolbar 1) at the top
 *   2. Active panel: header (title + toolbar 2) + content
 *   3. Footer (forge selector + actions), if any
 *
 * The left side-panel toggle lives in the workspace ribbon to the
 * left of this component; when collapsed, this whole component is
 * unmounted.
 */
function SidePanelView({ side, sidePanel, onActivate, onTogglePanel }: SidePanelViewProps) {
  return (
    <aside className="side-panel-preview" data-side={side}>
      <PanelSelector
        panels={sidePanel.panels}
        label={`${side === "left" ? "Left" : "Right"} side panel`}
        onSelect={onActivate}
      />
      <SidePanelBody side={side} sidePanel={sidePanel} onTogglePanel={onTogglePanel} />
    </aside>
  );
}

interface SidePanelBodyProps {
  side: "left" | "right";
  sidePanel: SidePanel;
  onTogglePanel: (panelId: string) => void;
}

/**
 * Body-only rendering: active panel + footer, no panel-selector.
 * Used by the right side panel where toolbar 1 lives in the
 * `right-side-panel-toolbar` row (next to the right toggle), not inside
 * the side-panel container itself.
 */
function SidePanelBody({ side, sidePanel, onTogglePanel }: SidePanelBodyProps) {
  const activePanel = sidePanel.panels.find((p) => p.visible) ?? null;

  return (
    <div className="side-panel-body" data-side={side}>
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
    </div>
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
