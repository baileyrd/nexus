import { useCallback, useEffect } from "react";
import {
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
} from "lucide-react";
import type { Panel, SidePanel } from "../../bindings";
import { useContentType } from "../../contributions";
import { useForgeStore } from "../../stores/forge";
import { useLayoutStore } from "../../stores/layout";
import { useBreakpoint, useBreakpointDownCross } from "../../util/breakpoints";
import { PanelSelector } from "./PanelSelector";
import { PanelToolbar } from "./PanelToolbar";
import { MenuBar } from "./MenuBar";
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

  // Responsive auto-collapse (PRD-07 §12.2). Fires only on downward
  // threshold crossings so manual expansions aren't undone on widen.
  const { name: breakpoint } = useBreakpoint();
  const leftCollapsed = layout?.leftSidePanel.collapsed ?? true;
  const rightCollapsed = layout?.rightSidePanel.collapsed ?? true;
  const collapseRightIfOpen = useCallback(() => {
    if (!rightCollapsed) toggleSidePanelCollapsed("right");
  }, [rightCollapsed, toggleSidePanelCollapsed]);
  const collapseLeftIfOpen = useCallback(() => {
    if (!leftCollapsed) toggleSidePanelCollapsed("left");
  }, [leftCollapsed, toggleSidePanelCollapsed]);
  useBreakpointDownCross(breakpoint, "md", collapseRightIfOpen);
  useBreakpointDownCross(breakpoint, "sm", collapseLeftIfOpen);

  return (
    <section className="workspace-view" data-breakpoint={breakpoint}>
      {error && <p className="error">Failed to load layout: {error}</p>}

      {layout ? (
        <div
          className="workspace-frame"
          data-workspace-name={layout.name}
          data-breakpoint={breakpoint}
        >
          <MenuBar />
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
              miniMode={layout.leftSidePanel.miniMode}
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
          <div
            className="right-side-panel"
            data-mini-mode={
              !layout.rightSidePanel.collapsed && layout.rightSidePanel.miniMode
                ? "true"
                : undefined
            }
          >
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
            {!layout.rightSidePanel.collapsed && !layout.rightSidePanel.miniMode && (
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
  const Glyph = side === "left"
    ? collapsed ? PanelLeftOpen : PanelLeftClose
    : collapsed ? PanelRightOpen : PanelRightClose;
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
      <Glyph size={16} aria-hidden="true" focusable="false" />
    </button>
  );
}

interface SidePanelViewProps {
  side: "left" | "right";
  sidePanel: SidePanel;
  onActivate: (panelId: string) => void;
  onTogglePanel: (panelId: string) => void;
  /** Icons-only rail (PRD-07 §5.1 / §8). When true, the panel body is
   *  omitted and only the selector row is rendered. */
  miniMode?: boolean;
}

/**
 * Left side panel — full height, stacked vertically:
 *
 *   1. Panel-selector toolbar (toolbar 1) at the top
 *   2. Active panel: header (title + toolbar 2) + content — omitted in mini-mode
 *   3. Footer (forge selector + actions), if any — omitted in mini-mode
 *
 * The left side-panel toggle lives in the workspace ribbon to the
 * left of this component; when collapsed, this whole component is
 * unmounted.
 */
function SidePanelView({ side, sidePanel, onActivate, onTogglePanel, miniMode }: SidePanelViewProps) {
  return (
    <aside
      className="side-panel-preview"
      data-side={side}
      data-mini-mode={miniMode ? "true" : undefined}
    >
      <PanelSelector
        panels={sidePanel.panels}
        label={`${side === "left" ? "Left" : "Right"} side panel`}
        onSelect={onActivate}
      />
      {!miniMode && (
        <SidePanelBody side={side} sidePanel={sidePanel} onTogglePanel={onTogglePanel} />
      )}
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
  const forgeName = useForgeStore((s) => s.info?.name);

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
        <SidePanelFooter footer={sidePanel.footer} forgeName={forgeName} />
      )}
    </div>
  );
}

interface PanelViewProps {
  panel: Panel;
  onTogglePanel: (panelId: string) => void;
}

function PanelView({ panel, onTogglePanel }: PanelViewProps) {
  const ContentComponent = useContentType(panel.contentType);
  return (
    <div className="panel-view" data-panel-id={panel.id}>
      <header className="panel-header">
        <span className="panel-title">{panel.title}</span>
        <PanelToolbar items={panel.toolbar} onTogglePanel={onTogglePanel} />
      </header>
      <div className="panel-content">
        {ContentComponent ? (
          <ContentComponent panel={panel} />
        ) : panel.contentType ? (
          <span className="panel-content-stub">
            no renderer for contentType: <code>{panel.contentType}</code>
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
