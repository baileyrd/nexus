import { useEffect } from "react";
import type { LayoutNode, Panel, Tab } from "../../bindings";
import { FileViewer } from "../panels/FileViewer";
import { useOpenFileStore } from "../../stores/openFile";
import { useOpenFile } from "../../stores/openFiles";
import { useLayoutStore } from "../../stores/layout";
import { useContentType } from "../../contributions/registry";
import { activateByContentType } from "../../plugins/scriptRuntime";
import { TabStrip } from "./TabStrip";

interface PaneViewProps {
  node: Extract<LayoutNode, { type: "leaf" }>;
  focused: boolean;
}

/** Parse `file:<relpath>` content-type, returning the relpath or null. */
function fileRelpathFromContentType(ct: string | undefined): string | null {
  if (!ct || !ct.startsWith("file:")) return null;
  return ct.slice("file:".length);
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
  const focusPane = useLayoutStore((s) => s.focusPane);

  const activeRelpath = fileRelpathFromContentType(activeTab?.contentType);
  // Legacy single-file consumers (Outline, plugin bridge) read from
  // useOpenFileStore. Keep that store in sync with the active file tab
  // without re-fetching from disk. Re-runs when the entry loads async.
  const activeEntry = useOpenFile(activeRelpath);
  useEffect(() => {
    if (!activeRelpath) return;
    if (activeEntry.file) {
      useOpenFileStore.getState().mirror(activeEntry.file, activeEntry.isDirty);
    }
  }, [activeRelpath, activeEntry.file, activeEntry.isDirty]);

  // Resolve a registered content-type component (for non-file tabs —
  // side panels, terminal, plugin surfaces). `file:<relpath>` tabs
  // short-circuit to FileViewer below.
  const ContentComponent = useContentType(
    activeRelpath ? null : activeTab?.contentType ?? null,
  );

  // UI F-3.2.1: activate any script plugin whose manifest declared
  // `on_content_type` for the active tab's content-type id.
  useEffect(() => {
    const ct = activeTab?.contentType;
    if (ct) activateByContentType(ct);
  }, [activeTab?.contentType]);

  // Keep the OLD global-file fallback working for the empty-layout
  // preset case: no file tab but `useOpenFileStore` holds something.
  const legacyOpenFile = useOpenFileStore((s) => s.file);

  const handleFocus = () => focusPane(node.id);

  return (
    <div
      className={focused ? "pane focused" : "pane"}
      onMouseDownCapture={handleFocus}
      onFocusCapture={handleFocus}
    >
      <TabStrip
        paneId={node.id}
        tabs={node.tabs}
        activeTabId={node.activeTabId}
      />
      <div className="pane-content">
        {activeRelpath && activeTab ? (
          <FileViewer relpath={activeRelpath} tabId={activeTab.id} />
        ) : ContentComponent && activeTab ? (
          <ContentComponent panel={tabAsPanel(activeTab)} />
        ) : legacyOpenFile ? (
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
