import type { LayoutNode, Tab } from "../../bindings";
import { TabStrip } from "./TabStrip";

interface PaneViewProps {
  node: Extract<LayoutNode, { type: "leaf" }>;
  focused: boolean;
}

export function PaneView({ node, focused }: PaneViewProps) {
  const activeTab: Tab | undefined = node.tabs.find(
    (t) => t.id === node.activeTabId,
  );

  return (
    <div className={focused ? "pane focused" : "pane"}>
      <TabStrip tabs={node.tabs} activeTabId={node.activeTabId} />
      <div className="pane-content">
        {activeTab ? (
          <PlaceholderSurface tab={activeTab} />
        ) : (
          <div className="pane-empty">
            <p className="label">Empty pane</p>
            <p className="hint">id · {node.id}</p>
          </div>
        )}
      </div>
    </div>
  );
}

function PlaceholderSurface({ tab }: { tab: Tab }) {
  // Stand-in until the editor / terminal / preview surfaces exist.
  return (
    <div className="placeholder" data-surface={tab.surface}>
      <p className="surface">{tab.surface}</p>
      <p className="content-type">{tab.contentType}</p>
    </div>
  );
}
