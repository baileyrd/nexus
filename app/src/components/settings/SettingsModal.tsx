import { useEffect, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useSettingsStore, type SettingsTab } from "../../stores/settings";
import { isCapturing } from "../../keybindings/capture-state";
import { Icon } from "../Icon";
import { GeneralTab } from "./tabs/GeneralTab";
import { HotkeysTab } from "./tabs/HotkeysTab";
import { PluginsTab } from "./tabs/PluginsTab";

interface TabDef {
  id: SettingsTab;
  title: string;
  icon: string;
  group: "options" | "plugins";
}

const TABS: readonly TabDef[] = [
  { id: "general", title: "General", icon: "settings", group: "options" },
  { id: "hotkeys", title: "Hotkeys", icon: "command", group: "options" },
  { id: "plugins", title: "Plugins", icon: "plug", group: "plugins" },
] as const;

/**
 * Two-pane Settings modal (PRD 07 §20). Mirrors the Obsidian settings
 * layout referenced in docs/references/obsidian-settings-modal.md:
 * a grouped left rail of tabs, a scrollable right pane for content.
 *
 * Opens via the `workspace.settings` contribution command. Esc and
 * backdrop click both close.
 */
export function SettingsModal() {
  const open = useSettingsStore((s) => s.open);
  const close = useSettingsStore((s) => s.closeSettings);

  if (!open) return null;
  return <SettingsDialog onClose={close} />;
}

function SettingsDialog({ onClose }: { onClose: () => void }) {
  const activeTab = useSettingsStore((s) => s.activeTab);
  const setActiveTab = useSettingsStore((s) => s.setActiveTab);

  // Close on global Escape — listening on the backdrop's keydown misses
  // Esc because focus lives inside the right-pane content. Suppressed
  // while a keybinding capture is in flight; Esc there cancels the
  // recording instead of closing the modal.
  useEffect(() => {
    function handler(e: KeyboardEvent) {
      if (e.key === "Escape" && !isCapturing()) {
        e.preventDefault();
        onClose();
      }
    }
    document.addEventListener("keydown", handler, true);
    return () => document.removeEventListener("keydown", handler, true);
  }, [onClose]);

  function handleKeyDown(_: ReactKeyboardEvent) {
    // Placeholder for future keyboard nav (arrow-key tab cycling).
  }

  const options = TABS.filter((t) => t.group === "options");
  const plugins = TABS.filter((t) => t.group === "plugins");

  return (
    <div
      className="settings-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        onKeyDown={handleKeyDown}
      >
        <nav className="settings-rail" aria-label="Settings sections">
          <RailSection title="Options" tabs={options} active={activeTab} onSelect={setActiveTab} />
          <RailSection title="Plugins" tabs={plugins} active={activeTab} onSelect={setActiveTab} />
        </nav>
        <div className="settings-content">
          <button
            type="button"
            className="settings-close"
            aria-label="Close settings"
            onClick={onClose}
          >
            <Icon name="x" size={18} />
          </button>
          {activeTab === "general" && <GeneralTab />}
          {activeTab === "hotkeys" && <HotkeysTab />}
          {activeTab === "plugins" && <PluginsTab />}
        </div>
      </div>
    </div>
  );
}

interface RailSectionProps {
  title: string;
  tabs: readonly TabDef[];
  active: SettingsTab;
  onSelect: (id: SettingsTab) => void;
}

function RailSection({ title, tabs, active, onSelect }: RailSectionProps) {
  if (tabs.length === 0) return null;
  return (
    <div className="settings-rail-section">
      <h4 className="settings-rail-title">{title}</h4>
      <ul className="settings-rail-list">
        {tabs.map((tab) => (
          <li key={tab.id}>
            <button
              type="button"
              className={
                tab.id === active
                  ? "settings-rail-item active"
                  : "settings-rail-item"
              }
              onClick={() => onSelect(tab.id)}
            >
              <Icon name={tab.icon} size={16} />
              <span>{tab.title}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
