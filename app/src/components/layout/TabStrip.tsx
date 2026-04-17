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
    />
  );
}

interface InnerProps {
  paneId: PaneId;
  tabs: Tab[];
  activeTabId: TabId | null | undefined;
  onActivate: (paneId: PaneId, tabId: string) => void;
  onClose: (tabId: string) => void;
}

function TabStripInner({
  paneId,
  tabs,
  activeTabId,
  onActivate,
  onClose,
}: InnerProps) {
  const scrollerRef = useRef<HTMLDivElement>(null);
  const tabRefs = useRef(new Map<string, HTMLDivElement>());
  const [overflowing, setOverflowing] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

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

  return (
    <div className="tab-strip" role="tablist">
      <div className="tab-strip-scroller" ref={scrollerRef}>
        {tabs.map((tab) => {
          const active = tab.id === activeTabId;
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
              className={active ? "tab active" : "tab"}
              onClick={() => onActivate(paneId, tab.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onActivate(paneId, tab.id);
                }
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
