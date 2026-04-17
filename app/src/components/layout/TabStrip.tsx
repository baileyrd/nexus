import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { PaneId, Tab, TabId } from "../../bindings";
import { useLayoutStore } from "../../stores/layout";

interface TabStripProps {
  paneId: PaneId;
  tabs: Tab[];
  activeTabId: TabId | null | undefined;
}

export function TabStrip({ paneId, tabs, activeTabId }: TabStripProps) {
  const setActiveTab = useLayoutStore((s) => s.setActiveTab);
  const closeTab = useLayoutStore((s) => s.closeTab);
  const reorderTabs = useLayoutStore((s) => s.reorderTabs);

  if (tabs.length === 0) {
    return <div className="tab-strip empty" aria-hidden>·</div>;
  }

  return (
    <TabStripInner
      paneId={paneId}
      tabs={tabs}
      activeTabId={activeTabId}
      onActivate={setActiveTab}
      onClose={closeTab}
      onReorder={reorderTabs}
    />
  );
}

interface InnerProps {
  paneId: PaneId;
  tabs: Tab[];
  activeTabId: TabId | null | undefined;
  onActivate: (paneId: PaneId, tabId: string) => void;
  onClose: (tabId: string) => void;
  onReorder: (paneId: PaneId, newTabIds: string[]) => void;
}

function TabStripInner({
  paneId,
  tabs,
  activeTabId,
  onActivate,
  onClose,
  onReorder,
}: InnerProps) {
  const scrollerRef = useRef<HTMLDivElement>(null);
  const tabRefs = useRef(new Map<string, HTMLDivElement>());
  const [overflowing, setOverflowing] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);

  // Recompute overflow state on tab-list change, container resize, or scroll.
  // A tab is "overflowing" when its rect sits outside the scroller's rect.
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;

    const compute = () => {
      const hidden =
        scroller.scrollWidth - scroller.clientWidth > 1; // 1px slop
      setOverflowing(hidden);
    };
    compute();

    const ro = new ResizeObserver(compute);
    ro.observe(scroller);
    scroller.addEventListener("scroll", compute, { passive: true });
    return () => {
      ro.disconnect();
      scroller.removeEventListener("scroll", compute);
    };
  }, [tabs.length]);

  // Keep the active tab visible after activation changes.
  useEffect(() => {
    if (!activeTabId) return;
    const el = tabRefs.current.get(activeTabId);
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }, [activeTabId]);

  // Close the overflow menu on outside click / escape.
  useEffect(() => {
    if (!menuOpen) return;
    const onDocMouseDown = (e: MouseEvent) => {
      const target = e.target as Node | null;
      if (!target) return;
      const menu = document.querySelector(".tab-overflow-menu");
      if (menu && menu.contains(target)) return;
      setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDocMouseDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDocMouseDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuOpen]);

  const handlePick = (tabId: string) => {
    onActivate(paneId, tabId);
    setMenuOpen(false);
    // Also scroll it into view in the strip.
    const el = tabRefs.current.get(tabId);
    el?.scrollIntoView({ block: "nearest", inline: "nearest" });
  };

  // Drag-to-reorder (PRD-07 §7.2). Native HTML5 DnD keeps the host
  // framework-free; the store's reorderTabs action enforces the
  // pinned-first invariant so a user can't drag a normal tab above a
  // pinned one.
  const handleDragStart = (e: React.DragEvent, tabId: string) => {
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("application/x-nexus-tab", tabId);
    setDraggingId(tabId);
  };
  const handleDragOver = (e: React.DragEvent, tabId: string) => {
    if (!draggingId || draggingId === tabId) return;
    const dragged = tabs.find((t) => t.id === draggingId);
    const target = tabs.find((t) => t.id === tabId);
    // Disallow crossing the pinned/unpinned boundary — the store would
    // re-partition anyway, but refusing the drop here makes the UX clear.
    if (!dragged || !target || dragged.pinned !== target.pinned) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDropTargetId(tabId);
  };
  const handleDrop = (e: React.DragEvent, tabId: string) => {
    e.preventDefault();
    const draggedId = draggingId ?? e.dataTransfer.getData("application/x-nexus-tab");
    setDraggingId(null);
    setDropTargetId(null);
    if (!draggedId || draggedId === tabId) return;
    const fromIdx = tabs.findIndex((t) => t.id === draggedId);
    const toIdx = tabs.findIndex((t) => t.id === tabId);
    if (fromIdx < 0 || toIdx < 0) return;
    const next = tabs.slice();
    const [moved] = next.splice(fromIdx, 1);
    if (!moved) return;
    next.splice(toIdx, 0, moved);
    onReorder(paneId, next.map((t) => t.id));
  };
  const handleDragEnd = () => {
    setDraggingId(null);
    setDropTargetId(null);
  };

  return (
    <div className="tab-strip" role="tablist">
      <div className="tab-strip-scroller" ref={scrollerRef}>
        {tabs.map((tab) => {
          const active = tab.id === activeTabId;
          const dragging = draggingId === tab.id;
          const dropTarget = dropTargetId === tab.id;
          const cls = [
            "tab",
            active ? "active" : null,
            dragging ? "dragging" : null,
            dropTarget ? "drop-target" : null,
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <div
              key={tab.id}
              ref={(el) => {
                if (el) tabRefs.current.set(tab.id, el);
                else tabRefs.current.delete(tab.id);
              }}
              role="tab"
              tabIndex={0}
              aria-selected={active}
              draggable
              className={cls}
              onClick={() => onActivate(paneId, tab.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onActivate(paneId, tab.id);
                }
              }}
              onDragStart={(e) => handleDragStart(e, tab.id)}
              onDragOver={(e) => handleDragOver(e, tab.id)}
              onDrop={(e) => handleDrop(e, tab.id)}
              onDragEnd={handleDragEnd}
              onDragLeave={() => {
                if (dropTargetId === tab.id) setDropTargetId(null);
              }}
            >
              <span className="label">{tab.label}</span>
              {tab.isDirty && (
                <span className="dirty" aria-label="unsaved changes">
                  •
                </span>
              )}
              {!tab.pinned && (
                <button
                  type="button"
                  className="tab-close"
                  aria-label={`Close ${tab.label}`}
                  title="Close tab"
                  onClick={(e) => {
                    e.stopPropagation();
                    onClose(tab.id);
                  }}
                  onMouseDown={(e) => e.stopPropagation()}
                >
                  ×
                </button>
              )}
            </div>
          );
        })}
      </div>
      {overflowing && (
        <div className="tab-overflow">
          <button
            type="button"
            className="tab-overflow-button"
            aria-label={`Show all ${tabs.length} tabs`}
            aria-haspopup="listbox"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((v) => !v)}
          >
            ▾
          </button>
          {menuOpen && (
            <ul className="tab-overflow-menu" role="listbox">
              {tabs.map((tab) => (
                <li
                  key={tab.id}
                  role="option"
                  aria-selected={tab.id === activeTabId}
                  className={tab.id === activeTabId ? "active" : ""}
                  onClick={() => handlePick(tab.id)}
                >
                  <span className="label">{tab.label}</span>
                  {tab.isDirty && <span className="dirty">•</span>}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
