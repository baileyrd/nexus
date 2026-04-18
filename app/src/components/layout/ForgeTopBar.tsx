import { useMemo } from "react";
import {
  Link2,
  PanelRight,
  Settings,
  SlidersHorizontal,
} from "lucide-react";
import { useForgeStore } from "../../stores/forge";
import { useOpenFileStore } from "../../stores/openFile";
import { useLayoutStore } from "../../stores/layout";
import { useSettingsStore } from "../../stores/settings";
import { ModeToggle } from "../ModeToggle";

/**
 * 36 px top bar matching the Nexus Forge design.
 *
 * Three columns:
 *   - Brand        (left, fixed 260 px)
 *   - Breadcrumb   (center, pill with sync pip)
 *   - Icon cluster (right, frameless utility buttons + ModeToggle)
 *
 * Data sources are the existing stores — no new state. The breadcrumb
 * composes `forge.info.name / <active-file> · md · Nw`, falling back to
 * "Workspace" when no file is open.
 */
export function ForgeTopBar() {
  const forgeName = useForgeStore((s) => s.info?.name);
  const activeFile = useOpenFileStore((s) => s.file);
  const toggleRight = useLayoutStore((s) => s.toggleSidePanelCollapsed);
  const openSettings = useSettingsStore((s) => s.openSettings);

  const breadcrumb = useMemo(() => {
    if (!activeFile) {
      return {
        head: forgeName ?? "Workspace",
        tail: "no file open",
        meta: null,
      };
    }
    const name = activeFile.name || activeFile.relpath;
    const ext = name.includes(".") ? name.split(".").pop() ?? "" : "";
    const stem = ext ? name.slice(0, -(ext.length + 1)) : name;
    const wordCount = countWords(activeFile.content ?? "");
    return {
      head: forgeName ?? "Workspace",
      tail: stem,
      meta: `${ext || "txt"} · ${formatWordCount(wordCount)}`,
    };
  }, [forgeName, activeFile]);

  return (
    <header className="forge-topbar" role="banner">
      <div className="forge-brand" aria-label="Nexus Forge">
        <span className="forge-mark" aria-hidden="true" />
      </div>
      <div
        className="forge-breadcrumb"
        role="navigation"
        aria-label="Active forge and file"
      >
        <span className="forge-sync-pip" aria-hidden="true" />
        <b>{breadcrumb.head}</b>
        <span className="forge-breadcrumb-sep">/</span>
        <span className="forge-breadcrumb-tail">{breadcrumb.tail}</span>
        {breadcrumb.meta && (
          <span className="forge-breadcrumb-meta">{breadcrumb.meta}</span>
        )}
      </div>
      <div className="forge-topbar-cluster" role="toolbar" aria-label="Workspace actions">
        <button
          type="button"
          className="forge-icon-btn"
          aria-label="Backlinks"
          title="Backlinks"
        >
          <Link2 size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="forge-icon-btn"
          aria-label="Tweaks"
          title="Tweaks"
          onClick={() => openSettings()}
        >
          <SlidersHorizontal size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="forge-icon-btn"
          aria-label="Toggle inspector"
          title="Toggle inspector"
          onClick={() => toggleRight("right")}
        >
          <PanelRight size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="forge-icon-btn"
          aria-label="Settings"
          title="Settings"
          onClick={() => openSettings()}
        >
          <Settings size={15} aria-hidden="true" />
        </button>
        <ModeToggle />
      </div>
    </header>
  );
}

function countWords(text: string): number {
  const matches = text.trim().match(/\S+/g);
  return matches ? matches.length : 0;
}

function formatWordCount(n: number): string {
  if (n >= 10000) return `${(n / 1000).toFixed(1)}kw`;
  if (n >= 1000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}kw`;
  return `${n}w`;
}
