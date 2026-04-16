import type { LayoutNode, Panel, Tab } from "../../bindings";
import { FileViewer } from "../panels/FileViewer";
import { useOpenFileStore } from "../../stores/openFile";
import { useContentType } from "../../contributions/registry";
import { TabStrip } from "./TabStrip";

interface PaneViewProps {
  node: Extract<LayoutNode, { type: "leaf" }>;
  focused: boolean;
}

/** Adapt a Tab's fields to the Panel shape expected by ContentComponent. */
function tabAsPanel(tab: Tab): Panel {
  return {
    id: tab.id,
    title: tab.label,
    icon: tab.icon ?? "file",
    visible: true,
    toolbar: [],
    contentType: tab.contentType,
  };
}

export function PaneView({ node, focused }: PaneViewProps) {
  const activeTab: Tab | undefined = node.tabs.find(
    (t) => t.id === node.activeTabId,
  );
  // Single global "open file" for now — the layout has no real tab
  // system yet, so we render the open file in any pane it's mounted in.
  // Multi-pane semantics arrive with PRD §7 / PRD-08.
  const openFile = useOpenFileStore((s) => s.file);
  // Resolve a registered content-type component for the active tab so
  // plugins can contribute tab surfaces (graph view, canvas editor, …)
  // through the same contribution registry used by side panels.
  const ContentComponent = useContentType(activeTab?.contentType ?? null);

  return (
    <div className={focused ? "pane focused" : "pane"}>
      <TabStrip tabs={node.tabs} activeTabId={node.activeTabId} />
      <div className="pane-content">
        {ContentComponent && activeTab ? (
          <ContentComponent panel={tabAsPanel(activeTab)} />
        ) : openFile ? (
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
