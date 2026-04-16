import { useEffect, useMemo, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { settingsTabKey, useSettingsStore } from "../../stores/settings";
import { isCapturing } from "../../keybindings/capture-state";
import { useSettingsTabs, type SettingsTab } from "../../contributions";
import { Icon } from "../Icon";
import { PluginSettingsTab } from "./tabs/PluginSettingsTab";

/** Rail-display shape: a subset of `SettingsTab` plus the synthetic
 *  entries we generate for plugin-contributed tabs from the backend. */
interface RailTab {
  id: string;
  title: string;
  icon: string;
  group: "options" | "plugins";
}

/**
 * Two-pane Settings modal (PRD 07 §20). Mirrors the Obsidian settings
 * layout referenced in docs/references/obsidian-settings-modal.md:
 * a grouped left rail of tabs, a scrollable right pane for content.
 *
 * Opens via the `workspace.settings` contribution command. Esc and
 * backdrop click both close.
 *
 * Tab content is resolved through the contribution registry
 * (`useSettingsTabs` → `contributions.registerSettingsTab`). Built-in
 * tabs register themselves at boot in `contributions/builtins.ts`;
 * plugin-contributed tabs still come from the backend via
 * `useSettingsStore.pluginTabs` and render through `PluginSettingsTab`.
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
  const pluginTabs = useSettingsStore((s) => s.pluginTabs);
  const registeredTabs = useSettingsTabs();

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

  const { options, plugins, activeRegistered, activePluginTab } = useMemo(() => {
    const options: RailTab[] = registeredTabs
      .filter((t) => t.group === "options")
      .map(toRailTab);
    const pluginBuiltin: RailTab[] = registeredTabs
      .filter((t) => t.group === "plugins")
      .map(toRailTab);
    const pluginContributed: RailTab[] = pluginTabs.map((t) => ({
      id: settingsTabKey(t),
      title: t.title,
      icon: t.icon,
      group: "plugins",
    }));
    const activeRegistered: SettingsTab | undefined = registeredTabs.find(
      (t) => t.id === activeTab,
    );
    const activePluginTab = pluginTabs.find(
      (t) => settingsTabKey(t) === activeTab,
    );
    return {
      options,
      plugins: [...pluginBuiltin, ...pluginContributed],
      activeRegistered,
      activePluginTab,
    };
  }, [activeTab, registeredTabs, pluginTabs]);

  const ActiveComponent = activeRegistered?.component;

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
          {ActiveComponent && <ActiveComponent />}
          {activePluginTab && <PluginSettingsTab tab={activePluginTab} />}
        </div>
      </div>
    </div>
  );
}

function toRailTab(t: SettingsTab): RailTab {
  return { id: t.id, title: t.title, icon: t.icon, group: t.group };
}

interface RailSectionProps {
  title: string;
  tabs: readonly RailTab[];
  active: string;
  onSelect: (id: string) => void;
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
