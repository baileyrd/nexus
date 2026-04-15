import type { LayoutNode, Tab } from "../../bindings";
import { FileViewer } from "../panels/FileViewer";
import { useOpenFileStore } from "../../stores/openFile";
import { TabStrip } from "./TabStrip";

interface PaneViewProps {
  node: Extract<LayoutNode, { type: "leaf" }>;
  focused: boolean;
}

export function PaneView({ node, focused }: PaneViewProps) {
  const activeTab: Tab | undefined = node.tabs.find(
    (t) => t.id === node.activeTabId,
  );
  // Single global "open file" for now — the layout has no real tab
  // system yet, so we render the open file in any pane it's mounted in.
  // Multi-pane semantics arrive with PRD §7 / PRD-08.
  const openFile = useOpenFileStore((s) => s.file);

  return (
    <div className={focused ? "pane focused" : "pane"}>
      <TabStrip tabs={node.tabs} activeTabId={node.activeTabId} />
      <div className="pane-content">
        {openFile ? (
          <FileViewer />
        ) : activeTab ? (
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
