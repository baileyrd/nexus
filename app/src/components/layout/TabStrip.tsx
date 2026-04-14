import type { Tab, TabId } from "../../bindings";

interface TabStripProps {
  tabs: Tab[];
  activeTabId: TabId | null | undefined;
}

export function TabStrip({ tabs, activeTabId }: TabStripProps) {
  if (tabs.length === 0) {
    return <div className="tab-strip empty" aria-hidden>·</div>;
  }
  return (
    <div className="tab-strip" role="tablist">
      {tabs.map((tab) => {
        const active = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={active}
            className={active ? "tab active" : "tab"}
          >
            <span className="label">{tab.label}</span>
            {tab.isDirty && (
              <span className="dirty" aria-label="unsaved changes">
                •
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}
