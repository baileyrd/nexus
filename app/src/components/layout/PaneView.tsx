import { useEffect } from "react";
import type { LayoutNode, Panel, Tab } from "../../bindings";
import { BaseFileView } from "../panels/BaseFileView";
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

/** Parse `base-file:<relpath>` content-type for PRD-10 base-directory
 *  tabs. Same pattern as `file:<relpath>` so a base tab slots into the
 *  existing layout machinery without a second dispatch arm. */
function baseRelpathFromContentType(ct: string | undefined): string | null {
  if (!ct || !ct.startsWith("base-file:")) return null;
  return ct.slice("base-file:".length);
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
  const activeBaseRelpath = baseRelpathFromContentType(activeTab?.contentType);
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
        ) : activeBaseRelpath ? (
          <BaseFileView relpath={activeBaseRelpath} />
        ) : ContentComponent && activeTab ? (
          <ContentComponent panel={tabAsPanel(activeTab)} />
        ) : legacyOpenFile ? (
          <FileViewer />
        ) : activeTab ? (
          <PlaceholderSurface tab={activeTab} />
        ) : (
          <WelcomeSurface paneId={node.id} />
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

/**
 * Rendered when a pane has no active tab — the Forge welcome doc.
 * Uses the editor's serif body styling so switching from a welcome to
 * an open doc feels continuous rather than a context switch.
 */
function WelcomeSurface({ paneId }: { paneId: string }) {
  return (
    <div className="pane-welcome" data-pane-id={paneId}>
      <article className="doc welcome-doc">
        <h1 className="title">Welcome to Forge.</h1>
        <p className="metaline">
          <span className="chip">Nexus</span>
          <span className="chip">workspace</span>
          <span className="tier">A place to think in public.</span>
        </p>
        <p>
          Forge is a microkernel editor shell. Every panel, command, and
          language feature you see is a plugin registered into a thin
          core — the same model that VS Code, IntelliJ, and Obsidian use
          to stay extensible without collapsing under their own weight.
        </p>
        <blockquote>
          Open a file from the tree on the left, or hit{" "}
          <code>⌘K</code> to summon the command palette.
        </blockquote>
        <h2>What's here</h2>
        <ul>
          <li>
            <b>Left.</b> Forge tree — the active workspace's notes,
            canvases, and bases.
          </li>
          <li>
            <b>Right.</b> Inspector — outline, backlinks, and the local
            graph for the current doc.
          </li>
          <li>
            <b>Bottom.</b> Status bar — sync state, git, index health,
            and live doc stats.
          </li>
        </ul>
        <h2>Getting started</h2>
        <ul>
          <li>
            Press <code>⌘K</code> to open the command palette; try{" "}
            <code>Open file</code>, <code>Switch forge</code>, or{" "}
            <code>Switch layout</code>.
          </li>
          <li>
            Right-click the tree for <code>New note</code>,{" "}
            <code>New folder</code>, and <code>Rename</code>.
          </li>
          <li>
            Every view is a plugin — install more from{" "}
            <code>Settings → Plugins</code>.
          </li>
        </ul>
      </article>
    </div>
  );
}
