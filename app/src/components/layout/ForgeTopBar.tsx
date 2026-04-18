import { useMemo } from "react";
import {
  Folder,
  Link2,
  Minus,
  PanelRight,
  Search,
  Settings,
  SlidersHorizontal,
  Square,
  Star,
  X,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useForgeStore } from "../../stores/forge";
import { useOpenFileStore } from "../../stores/openFile";
import { useLayoutStore } from "../../stores/layout";
import { useSettingsStore } from "../../stores/settings";
import { usePaletteStore } from "../../stores/palette";
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
  const toggleSide = useLayoutStore((s) => s.toggleSidePanelCollapsed);
  const openSettings = useSettingsStore((s) => s.openSettings);
  const openPalette = usePaletteStore((s) => s.openPalette);

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
    <header
      className="forge-topbar"
      role="banner"
      data-tauri-drag-region
    >
      <div
        className="forge-topbar-left"
        role="toolbar"
        aria-label="Quick views"
      >
        <button
          type="button"
          className="forge-icon-btn"
          title="Toggle file tree"
          aria-label="Toggle file tree"
          onClick={() => toggleSide("left")}
        >
          <Folder size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="forge-icon-btn"
          title="Search (⌘P)"
          aria-label="Search"
          onClick={() => openPalette()}
        >
          <Search size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="forge-icon-btn"
          title="Favorites"
          aria-label="Favorites"
        >
          <Star size={15} aria-hidden="true" />
        </button>
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
          onClick={() => toggleSide("right")}
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
        <WindowControls />
      </div>
    </header>
  );
}

/**
 * Custom client-side min / maximize / close buttons. Wired to the
 * active Tauri window via `@tauri-apps/api/window`. Rendered only
 * under Forge (the decorations: false config removes the OS
 * chrome); other themes / the web preview fall through harmlessly
 * because the calls fail with a warning instead of crashing.
 */
function WindowControls() {
  return (
    <div className="forge-window-controls" aria-label="Window controls">
      <button
        type="button"
        className="forge-icon-btn"
        title="Minimize"
        aria-label="Minimize"
        onClick={() => {
          void getCurrentWindow().minimize();
        }}
      >
        <Minus size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        className="forge-icon-btn"
        title="Maximize"
        aria-label="Maximize"
        onClick={() => {
          void getCurrentWindow().toggleMaximize();
        }}
      >
        <Square size={12} aria-hidden="true" />
      </button>
      <button
        type="button"
        className="forge-icon-btn forge-icon-btn--close"
        title="Close"
        aria-label="Close"
        onClick={() => {
          void getCurrentWindow().close();
        }}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
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
