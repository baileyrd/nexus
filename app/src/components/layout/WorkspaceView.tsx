import { useEffect } from "react";
import { useLayoutStore } from "../../stores/layout";
import { LayoutPresetPicker } from "./LayoutPresetPicker";
import { SplitPane } from "./SplitPane";

export function WorkspaceView() {
  const layout = useLayoutStore((s) => s.layout);
  const load = useLayoutStore((s) => s.load);
  const loading = useLayoutStore((s) => s.loading);
  const error = useLayoutStore((s) => s.error);

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
            <SidebarPreview side="left" panels={layout.leftSidebar.panels} />
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
            <SidebarPreview side="right" panels={layout.rightSidebar.panels} />
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
  panels: { id: string; title: string; icon: string }[];
}

function SidebarPreview({ side, panels }: SidebarPreviewProps) {
  return (
    <aside className="sidebar-preview" data-side={side}>
      <ul>
        {panels.map((p) => (
          <li key={p.id}>
            <span className="icon" aria-hidden>
              ◇
            </span>
            <span className="title">{p.title}</span>
          </li>
        ))}
        {panels.length === 0 && <li className="empty">no panels</li>}
      </ul>
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
